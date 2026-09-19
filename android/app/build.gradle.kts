plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "org.racnet.android"
    compileSdk = 35

    defaultConfig {
        applicationId = "org.racnet.android"
        // L2CAP CoC (listen/createInsecureL2capChannel) is API 29+; the
        // transport is the app's reason to exist (ADR-0016).
        minSdk = 29
        targetSdk = 35
        versionCode = 2
        versionName = "0.2.0-preview"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
    testOptions {
        unitTests.all {
            it.systemProperty("jna.library.path", rootProject.file("../target/release").absolutePath)
        }
    }
    buildFeatures {
        compose = true
    }
    lint {
        // Full results in CI stdout, not just the first failure.
        textReport = true
    }
}

// Dependency justifications: ADR-0016.
dependencies {
    implementation(platform("androidx.compose:compose-bom:2024.12.01"))
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("androidx.lifecycle:lifecycle-service:2.8.7")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.1")
    // JNA carries the JNI surface required by UniFFI's Kotlin bindings.
    implementation("net.java.dev.jna:jna:5.15.0@aar")

    testRuntimeOnly("net.java.dev.jna:jna:5.15.0@jar")
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.10.1")
}

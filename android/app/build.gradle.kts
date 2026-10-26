import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}

val signing = Properties().apply {
    val file = rootProject.file("signing/keystore.properties")
    if (file.exists()) file.inputStream().use(::load)
}

android {
    namespace = "com.crosbic.phonemedia"
    compileSdk = 37

    defaultConfig {
        applicationId = "com.crosbic.phonemedia"
        minSdk = 30
        targetSdk = 37
        versionCode = 10100
        versionName = "1.1"
    }

    buildFeatures {
        compose = true
    }

    signingConfigs {
        if (!signing.isEmpty) {
            create("release") {
                storeFile = rootProject.file(signing.getProperty("storeFile"))
                storePassword = signing.getProperty("storePassword")
                keyAlias = signing.getProperty("keyAlias")
                keyPassword = signing.getProperty("keyPassword")
            }
        }
    }

    buildTypes {
        getByName("release") {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"))
            signingConfig = signingConfigs.findByName("release")
        }
    }
}

dependencies {
    implementation(platform("androidx.compose:compose-bom:2026.09.00"))
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.activity:activity-compose:1.13.0")
}

import java.util.Properties

plugins {
    id("com.android.application")
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
        versionCode = 10000
        versionName = "1.0"
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
            signingConfig = signingConfigs.findByName("release")
        }
    }
}

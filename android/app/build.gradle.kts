plugins {
  id("com.android.application")
  kotlin("android")
}

android {
  namespace = "dev.flexdeck.app"
  compileSdk = 34

  defaultConfig {
    applicationId = "dev.flexdeck"
    // The tablets this exists for run old Android, and the renderer is plain
    // Canvas drawing with no modern API surface, so an old floor costs nothing.
    minSdk = 21
    targetSdk = 34
    // CI stamps the release's version (`-PflexdeckVersionName=0.2.0
    // -PflexdeckVersionCode=200`); a local build keeps these defaults.
    versionCode = (project.findProperty("flexdeckVersionCode") as String?)?.toInt() ?: 1
    versionName = (project.findProperty("flexdeckVersionName") as String?) ?: "0.1.0"
  }

  // The release key comes from the environment, never the repository. CI writes
  // the keystore from a secret; without it a release build is simply unsigned.
  val keystore = System.getenv("FLEXDECK_KEYSTORE")
  if (keystore != null) {
    signingConfigs {
      create("release") {
        storeFile = file(keystore)
        storePassword = System.getenv("FLEXDECK_KEYSTORE_PASSWORD")
        keyAlias = System.getenv("FLEXDECK_KEY_ALIAS")
        keyPassword = System.getenv("FLEXDECK_KEY_PASSWORD")
      }
    }
  }

  buildTypes {
    // A debug build installs beside the release one: its own package, its own
    // launcher name, and the debug key, which the release package would reject.
    debug {
      applicationIdSuffix = ".dev"
      versionNameSuffix = "-dev"
      resValue("string", "app_name", "flexdeck dev")
    }
    release {
      resValue("string", "app_name", "flexdeck")
      signingConfig = signingConfigs.findByName("release")
      // Turning these on is what makes a release build worth shipping: an
      // unshrunk APK is more code to load, more to verify, and more surface for
      // the platform to page in than the app actually uses. The keep rules come
      // from `proguard-rules.pro` plus the consumer rules OkHttp and
      // kotlinx-serialization ship themselves.
      isMinifyEnabled = true
      isShrinkResources = true
      proguardFiles(
        getDefaultProguardFile("proguard-android-optimize.txt"),
        "proguard-rules.pro",
      )
    }
  }

  // One APK per CPU family plus a universal one that runs everywhere; the
  // download page's plain `flexdeck-android.apk` is the universal build.
  splits {
    abi {
      isEnable = true
      reset()
      include("arm64-v8a", "armeabi-v7a", "x86_64", "x86")
      isUniversalApk = true
    }
  }

  compileOptions {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
  }

  kotlinOptions {
    jvmTarget = "17"
  }
}

dependencies {
  implementation(project(":core:client"))
  implementation(project(":core:grid"))

  // WebSockets. Hand-rolling the upgrade and framing would be a large amount of
  // code to get subtly wrong, and this is the one dependency the client cannot
  // reasonably avoid.
  implementation("com.squareup.okhttp3:okhttp:5.5.1")
}

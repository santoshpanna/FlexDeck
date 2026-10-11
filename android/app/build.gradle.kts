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
    versionCode = 1
    versionName = "0.1.0"
  }

  buildTypes {
    release {
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
  implementation("com.squareup.okhttp3:okhttp:5.5.0")
}

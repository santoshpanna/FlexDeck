pluginManagement {
  repositories {
    gradlePluginPortal()
    mavenCentral()
    google()
  }
}

dependencyResolutionManagement {
  repositories {
    mavenCentral()
    google()
  }
}

rootProject.name = "flexdeck-android"

// Pure Kotlin/JVM. Deliberately NOT an Android library: the grid algorithm is
// plain integer maths with no Android dependency, so it can be built and tested
// on any JVM — including CI — without the Android SDK installed.
include(":core:grid")

// The client's protocol and connection logic, also pure Kotlin/JVM for the same
// reason. Keeping it out of the Android module means it is testable on a machine
// with no Android SDK, and that the tablet UI is left with nothing but drawing
// and event plumbing.
include(":core:client")

include(":app")

plugins {
  kotlin("jvm")
}

kotlin {
  jvmToolchain(17)
}

dependencies {
  // The grid algorithm is shared with the portal and the agent, so the tablet's
  // rectangles are computed by the same code rather than a lookalike.
  api(project(":core:grid"))

  // JSON, for applying the patches the agent sends. kotlinx-serialization's
  // JsonElement API is used directly rather than generated @Serializable
  // classes, because the wire format is the proto3 JSON mapping and its
  // *omissions* are the interesting part — see Frames.kt.
  // `api`, not `implementation`: this module's public surface exposes JsonObject,
  // so a consumer that touches a DeckSession needs the library on its compile
  // classpath. Hiding it would only move the failure to whoever tried.
  api("org.jetbrains.kotlinx:kotlinx-serialization-json:1.7.3")

  testImplementation(kotlin("test"))
}

tasks.test {
  useJUnitPlatform()
  testLogging {
    events("passed", "failed", "skipped")
  }
}

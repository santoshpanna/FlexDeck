plugins {
  kotlin("jvm")
}

kotlin {
  jvmToolchain(17)
}

dependencies {
  // Test-only. The runtime module itself has no dependencies at all, which
  // matters on a 1 GB tablet.
  testImplementation(kotlin("test"))
  testImplementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.11.0")
}

tasks.test {
  useJUnitPlatform()
  testLogging {
    events("passed", "failed", "skipped")
  }
}

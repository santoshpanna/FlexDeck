package dev.flexdeck.grid

import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/**
 * Cross-renderer parity: Kotlin must reproduce the committed golden fixtures
 * exactly.
 *
 * The golden `*.expected.json` fixtures in `contract/fixtures/grid` are
 * generated from the TypeScript reference implementation and are also checked
 * by the Rust suite. If this fails while the TypeScript test passes, the tablet
 * renderer has drifted from the web preview.
 */
class GridLayoutParityTest {

  @Test
  fun `at least one fixture exists`() {
    assertTrue(fixtureFiles().isNotEmpty(), "no fixtures found in ${findFixturesDir()}")
  }

  @Test
  fun `every fixture matches the golden expectation`() {
    val files = fixtureFiles()
    for (file in files) {
      val fixture = Json.parseToJsonElement(file.readText()).jsonObject
      val name = fixture["name"]!!.jsonPrimitive.content
      val spec = specFrom(fixture["spec"]!!.jsonObject)
      val widgets = widgetsFrom(fixture["widgets"]!!.jsonArray)

      assertMatches(name, spec, widgets, file, ".expected.json")

      // The same fixture on a panel held the other way up. Every one of these
      // goldens moves if the transpose changes, so it cannot drift between the
      // web preview and this renderer.
      val portrait = GridLayout.orientPage(spec, widgets, Orientation.PORTRAIT)
      assertMatches(
        "$name (portrait)",
        portrait.spec,
        portrait.widgets,
        file,
        ".portrait.expected.json",
      )
    }
    println("verified ${files.size} fixture(s) in both orientations against golden expectations")
  }

  ///
  /// Check one oriented view of a fixture against its golden.
  ///
  /// Factored out because landscape and portrait differ ONLY in which transform is
  /// applied first, and a second hand-written copy of these assertions is exactly
  /// how a parity test quietly stops covering one of the two.
  ///
  private fun assertMatches(
    name: String,
    spec: GridSpec,
    widgets: List<WidgetSpan>,
    fixtureFile: File,
    suffix: String,
  ) {
    val expectedFile = File(fixtureFile.parentFile, fixtureFile.name.removeSuffix(".json") + suffix)
    assertTrue(expectedFile.isFile, "missing ${expectedFile.name}")
    val expected = Json.parseToJsonElement(expectedFile.readText()).jsonObject

    val geometry = GridLayout.computeGrid(spec)
    val cells = expected["cells"]!!.jsonObject
    assertEquals(intsFrom(cells["x"]!!.jsonArray), geometry.cols.offsets, "$name: column offsets")
    assertEquals(intsFrom(cells["w"]!!.jsonArray), geometry.cols.sizes, "$name: column sizes")
    assertEquals(intsFrom(cells["y"]!!.jsonArray), geometry.rows.offsets, "$name: row offsets")
    assertEquals(intsFrom(cells["h"]!!.jsonArray), geometry.rows.sizes, "$name: row sizes")

    val expectedRects = expected["rects"]!!.jsonObject
    val actualRects = GridLayout.layoutPage(spec, widgets)
    assertEquals(expectedRects.keys, actualRects.keys, "$name: widget id set")
    for ((id, rectJson) in expectedRects) {
      assertEquals(rectFrom(rectJson.jsonObject), actualRects[id], "$name: rect for '$id'")
    }
  }

  private fun specFrom(obj: JsonObject) =
    GridSpec(
      cols = obj["cols"]!!.jsonPrimitive.int,
      rows = obj["rows"]!!.jsonPrimitive.int,
      gapX = obj["gapX"]!!.jsonPrimitive.int,
      gapY = obj["gapY"]!!.jsonPrimitive.int,
      usableW = obj["usableW"]!!.jsonPrimitive.int,
      usableH = obj["usableH"]!!.jsonPrimitive.int,
    )

  private fun widgetsFrom(array: JsonArray) =
    array.map { element ->
      val obj = element.jsonObject
      WidgetSpan(
        id = obj["id"]!!.jsonPrimitive.content,
        col = obj["col"]!!.jsonPrimitive.int,
        row = obj["row"]!!.jsonPrimitive.int,
        w = obj["w"]!!.jsonPrimitive.int,
        h = obj["h"]!!.jsonPrimitive.int,
      )
    }

  private fun rectFrom(obj: JsonObject) =
    Rect(
      x = obj["x"]!!.jsonPrimitive.int,
      y = obj["y"]!!.jsonPrimitive.int,
      w = obj["w"]!!.jsonPrimitive.int,
      h = obj["h"]!!.jsonPrimitive.int,
    )

  private fun intsFrom(array: JsonArray) = array.map { it.jsonPrimitive.int }

  private fun fixtureFiles(): List<File> =
    findFixturesDir()
      .listFiles { file -> file.name.endsWith(".json") && !file.name.endsWith(".expected.json") }
      ?.sorted()
      ?: emptyList()

  /** Walk up from the working directory so this works regardless of Gradle's cwd. */
  private fun findFixturesDir(): File {
    var dir: File? = File(System.getProperty("user.dir")).absoluteFile
    while (dir != null) {
      val candidate = File(dir, "contract/fixtures/grid")
      if (candidate.isDirectory) return candidate
      dir = dir.parentFile
    }
    error("could not locate contract/fixtures/grid from ${System.getProperty("user.dir")}")
  }
}

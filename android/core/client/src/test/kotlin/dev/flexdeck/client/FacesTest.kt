package dev.flexdeck.client

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.io.File
import kotlin.math.abs
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class FacesTest {

    /** The contract's fixtures, found from wherever Gradle runs the test. */
    private fun fixture(name: String): JsonObject {
        var dir: File? = File("").absoluteFile
        while (dir != null && !File(dir, "contract/fixtures/face").isDirectory) dir = dir.parentFile
        val file = File(dir ?: error("no contract/fixtures/face above ${File("").absolutePath}"), "contract/fixtures/face/$name")
        return Json.parseToJsonElement(file.readText()) as JsonObject
    }

    @Test
    fun `every derive fixture gives its face`() {
        for (case in fixture("derive.json")["cases"] as JsonArray) {
            val c = case.jsonObject
            val look = (c["look"] as? JsonPrimitive)?.content
            assertEquals(c["face"], Faces.faceFor(c["widget"]!!.jsonObject, look), c["name"]!!.jsonPrimitive.content)
        }
    }

    @Test
    fun `every place of every layout is where the reference puts it`() {
        for (key in fixture("layout.expected.json")["keys"] as JsonArray) {
            val k = key.jsonObject
            val w = k["w"]!!.jsonPrimitive.content.toDouble()
            val h = k["h"]!!.jsonPrimitive.content.toDouble()
            val unit = k["unit"]!!.jsonPrimitive.content.toDouble()
            for ((layout, places) in k["layouts"]!!.jsonObject) {
                val ours = FaceLayout.place(layout, w, h, unit)
                for ((slot, expected) in places.jsonObject) {
                    val e = expected.jsonObject
                    val got = ours.getValue(slot)
                    for ((field, value) in listOf("x" to got.x, "y" to got.y, "w" to got.w, "h" to got.h)) {
                        val want = e[field]!!.jsonPrimitive.content.toDouble()
                        assertTrue(abs(want - value) < 0.011, "$layout $slot $field on ${w}x$h: $value, not $want")
                    }
                }
            }
        }
    }

    @Test
    fun `a strip of zones falls where the reference puts it`() {
        for (key in fixture("layout.expected.json")["keys"] as JsonArray) {
            val k = key.jsonObject
            val w = k["w"]!!.jsonPrimitive.content.toDouble()
            val h = k["h"]!!.jsonPrimitive.content.toDouble()
            val unit = k["unit"]!!.jsonPrimitive.content.toDouble()
            val want = k["zones"]!!.jsonObject
            val ours = FaceLayout.placeZones(3, w, h, unit)
            val expected = listOf(want["face"]!!.jsonObject) + (want["zones"] as JsonArray).map { it.jsonObject }
            for ((got, e) in (listOf(ours.face) + ours.zones).zip(expected)) {
                for ((field, value) in listOf("x" to got.x, "y" to got.y, "w" to got.w, "h" to got.h)) {
                    val v = e[field]!!.jsonPrimitive.content.toDouble()
                    assertTrue(abs(v - value) < 0.011, "zone $field on ${w}x$h: $value, not $v")
                }
            }
        }
    }

    @Test
    fun `an element put anywhere falls where the reference puts it`() {
        for (key in fixture("layout.expected.json")["keys"] as JsonArray) {
            val k = key.jsonObject
            val w = k["w"]!!.jsonPrimitive.content.toDouble()
            val h = k["h"]!!.jsonPrimitive.content.toDouble()
            val want = k["box"]!!.jsonObject
            val got = FaceLayout.placeBox(Box(0.1, 0.25, 0.55, 0.333), w, h)
            for ((field, value) in listOf("x" to got.x, "y" to got.y, "w" to got.w, "h" to got.h)) {
                val v = want[field]!!.jsonPrimitive.content.toDouble()
                assertTrue(abs(v - value) < 0.011, "box $field on ${w}x$h: $value, not $v")
            }
        }
        val read = FaceReader.read(Json.parseToJsonElement("""{"elements":[{"box":{"x":0.1,"y":0.2,"w":0.3,"h":0.4}},{}]}""").jsonObject)
        assertEquals(Box(0.1, 0.2, 0.3, 0.4), read.elements[0].box)
        assertEquals(null, read.elements[1].box)
    }

    @Test
    fun `a countdown reads as the reference shows it`() {
        for (case in fixture("countdown.json")["cases"] as JsonArray) {
            val pair = case as JsonArray
            val seconds = (pair[0] as? JsonPrimitive)?.content?.toDoubleOrNull()
            assertEquals(pair[1].jsonPrimitive.content, Clock.countdown(seconds), "$seconds")
        }
        assertEquals(30.0, Clock.timerLeft(40.0, true, 1_000, 11_000))
        assertEquals(40.0, Clock.timerLeft(40.0, false, 1_000, 11_000))
        assertEquals(0.0, Clock.timerLeft(5.0, true, 0, 60_000))
    }

    @Test
    fun `zones are read, and their flows never make the key wait`() {
        val widget = Json.parseToJsonElement(
            """
            {"id":"m","type":"WIDGET_TYPE_BUTTON","col":0,"row":0,"w":3,"h":1,
             "face":{"zones":[{"name":"prev","icon":"skip-back"},{"name":"play","icon":"play","iconOn":"pause"},{"name":""}]},
             "flows":[{"trigger":{"gesture":"GESTURE_SHORT_PRESS"}},{"trigger":{"gesture":"GESTURE_LONG_PRESS"},"zone":"play"}]}
            """.trimIndent(),
        ).jsonObject
        val read = DeckReader.readWidget(widget)
        assertEquals(listOf(ZoneSpec("prev", "skip-back"), ZoneSpec("play", "play", "pause")), read.zones)
        assertEquals(setOf("GESTURE_SHORT_PRESS"), read.gestures)
    }

    @Test
    fun `a face reads into what drawing needs, defaults filled`() {
        val spec = FaceReader.read(
            Json.parseToJsonElement(
                """
                {"layout":"FACE_LAYOUT_DIAL","background":{"graph":"cpu","dim":200,"fit":"IMAGE_FIT_CONTAIN"},
                 "elements":[{"kind":"ELEMENT_KIND_GAUGE","slot":"SLOT_VISUAL","template":"{{cpu}}","max":50},
                             {"template":"CPU","text":{"size":"TEXT_SIZE_L","bold":false}},
                             {"kind":"ELEMENT_KIND_CLOCK","clock":{"analog":true,"zone":"Asia/Tokyo"}}]}
                """.trimIndent(),
            ) as JsonObject,
        )
        assertEquals("FACE_LAYOUT_DIAL", spec.layout)
        assertEquals(90, spec.background.dim, "clamped")
        assertTrue(spec.background.contain)
        assertEquals(50.0, spec.elements[0].max)
        assertTrue(spec.elements[0].live)
        assertEquals("SLOT_TITLE", spec.elements[1].slot)
        assertEquals(false, spec.elements[1].bold)
        assertEquals(null, spec.elements[0].bold, "unset: the place decides")
        assertTrue(spec.elements[2].analog)
        assertEquals("Asia/Tokyo", spec.elements[2].zone)
    }

    @Test
    fun `a fade reads its angle and shape, unset being top to bottom`() {
        fun read(background: String) = FaceReader.read(Json.parseToJsonElement("""{"background":$background}""").jsonObject).background
        assertEquals(180, read("""{"color":"#111111","colorTo":"#222222"}""").angle)
        assertEquals(135, read("""{"color":"#111111","colorTo":"#222222","angle":135}""").angle)
        assertEquals(0, read("""{"color":"#111111","colorTo":"#222222","angle":360}""").angle, "straight up is written 360")
        assertTrue(read("""{"color":"#111111","colorTo":"#222222","gradient":"GRADIENT_RADIAL"}""").radial)
    }

    @Test
    fun `a fade runs corner to corner the way CSS draws it`() {
        fun near(expected: FloatArray, actual: FloatArray) =
            expected.zip(actual).forEach { (e, a) -> assertTrue(abs(e - a) < 0.01f, "${expected.toList()} vs ${actual.toList()}") }
        near(floatArrayOf(50f, 0f, 50f, 40f), Fade.line(100f, 40f, 180))
        near(floatArrayOf(50f, 40f, 50f, 0f), Fade.line(100f, 40f, 0))
        near(floatArrayOf(0f, 20f, 100f, 20f), Fade.line(100f, 40f, 90))
        // 45° on a square: bottom left to top right.
        near(floatArrayOf(0f, 100f, 100f, 0f), Fade.line(100f, 100f, 45))
        assertTrue(abs(Fade.radius(60f, 80f) - 50f) < 0.01f)
    }
}

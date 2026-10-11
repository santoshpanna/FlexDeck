package dev.flexdeck.client

import java.io.File
import kotlin.math.abs
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

class IconsTest {

    @Test
    fun `moves, lines, relative steps and close come out absolute`() {
        val segments = PathData.parse("M4 9h3l4-4v14l-4-4H4z")
        assertEquals(
            listOf(
                Segment.Move(4f, 9f),
                Segment.Line(7f, 9f),
                Segment.Line(11f, 5f),
                Segment.Line(11f, 19f),
                Segment.Line(7f, 15f),
                Segment.Line(4f, 15f),
                Segment.Close,
            ),
            segments,
        )
    }

    @Test
    fun `numbers run together the way SVG writes them`() {
        assertEquals(
            listOf(Segment.Move(1.5f, 0.5f), Segment.Line(-1f, -2f)),
            PathData.parse("M1.5.5L-1-2"),
        )
    }

    @Test
    fun `a full circle drawn as two arcs ends where it started and stays round`() {
        val segments = PathData.parse("M8.5 12a3.5 3.5 0 1 0 7 0a3.5 3.5 0 1 0 -7 0")
        val ends = segments.filterIsInstance<Segment.Cubic>()
        assertTrue(ends.size >= 4, "each half-turn is at least two quarter curves")
        val last = ends.last()
        assertEquals(8.5f, last.x, 1e-3f)
        assertEquals(12f, last.y, 1e-3f)
        for (curve in ends) {
            val distance = kotlin.math.hypot(curve.x - 12.0, curve.y - 12.0)
            assertTrue(abs(distance - 3.5) < 1e-3, "every joint lies on the circle: $distance")
        }
    }

    @Test
    fun `an icon value is a bundled name or a hashed picture`() {
        assertEquals(Icons.Source.Bundled("mic"), Icons.source("mic"))
        val sha = "ab".repeat(32)
        assertEquals(Icons.Source.Image(sha), Icons.source("img:$sha"))
        assertNull(Icons.source("img:../../etc"), "only a real hash is fetched")
        assertNull(Icons.source("  "))
    }

    @Test
    fun `every bundled icon parses into something drawable`() {
        val asset = File("../../app/src/main/assets/icons.json")
        val icons = Icons.read(asset.readText())
        assertTrue(icons.size > 30, "${icons.size} icons")
        for ((name, paths) in icons) {
            for (path in paths) {
                assertTrue(PathData.parse(path).isNotEmpty(), "$name: $path")
            }
        }
    }

    @Test
    fun `a pack's icon is named and a pack reads into pictures and paths`() {
        assertEquals(Icons.Source.Pack("retro-keys", "Play"), Icons.source("pack:retro-keys/Play"))
        assertEquals(null, Icons.source("pack:retro-keys"))
        val pack = Icons.readPack(
            """{"id":"p","icons":[{"name":"Play","image":"img:ab"},
               {"name":"clock","paths":["M0 0L1 1"],"view":[0,-960,960,960],"fill":true},
               {"name":"line","paths":["M0 0"],"stroke":1.5}]}""",
        )
        assertEquals("img:ab", pack["Play"]?.image)
        assertEquals(listOf(0f, -960f, 960f, 960f), pack["clock"]?.view)
        assertTrue(pack["clock"]!!.fill)
        assertEquals(listOf(0f, 0f, 24f, 24f), pack["line"]?.view, "no view: 24 by 24")
        assertEquals(1.5f, pack["line"]?.stroke)
        assertTrue(Icons.readPack("not json").isEmpty())
    }

    @Test
    fun `quadratic and smooth curves become cubics`() {
        // Q from (0,0) via (3,3) to (6,0): controls two thirds of the way.
        val q = PathData.parse("M0 0Q3 3 6 0")
        assertEquals(Segment.Cubic(2f, 2f, 4f, 2f, 6f, 0f), q[1])
        // T mirrors the last quadratic's control: (9,-3) for the next from (6,0).
        val t = PathData.parse("M0 0Q3 3 6 0T12 0")
        assertEquals(Segment.Cubic(8f, -2f, 10f, -2f, 12f, 0f), t[2])
        // S mirrors the last cubic's second control.
        val smooth = PathData.parse("M0 0C1 1 2 1 3 0s2 -1 3 0")
        assertEquals(Segment.Cubic(4f, -1f, 5f, -1f, 6f, 0f), smooth[2])
        // Material-style relative quadratics parse to the end.
        assertEquals(4, PathData.parse("M480-80q-83 0-156-31t-127-86z").size)
    }
}

package dev.flexdeck.client

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class ScreenCareTest {

    @Test
    fun `a dim window can span midnight`() {
        val night = ScreenCare(dim = true, dimFrom = 22 * 60, dimTo = 7 * 60)
        assertTrue(night.dimmedAt(23 * 60))
        assertTrue(night.dimmedAt(3 * 60))
        assertFalse(night.dimmedAt(12 * 60))
        assertFalse(night.dimmedAt(7 * 60), "the end is not included")
        val afternoon = ScreenCare(dim = true, dimFrom = 13 * 60, dimTo = 14 * 60)
        assertTrue(afternoon.dimmedAt(13 * 60 + 30))
        assertFalse(afternoon.copy(dim = false).dimmedAt(13 * 60 + 30))
        assertFalse(ScreenCare(dim = true, dimFrom = 60, dimTo = 60).dimmedAt(60), "an empty window dims nothing")
    }

    @Test
    fun `the shift walks around the centre and never strays more than a step`() {
        val seen = (0 until 9).map { ScreenCare.offset(it * ScreenCare.SHIFT_MS, 2f) }
        assertEquals(9, seen.toSet().size, "every place once a cycle")
        assertTrue(seen.all { (x, y) -> kotlin.math.abs(x) <= 2f && kotlin.math.abs(y) <= 2f })
        assertEquals(seen[0], ScreenCare.offset(9 * ScreenCare.SHIFT_MS, 2f), "and round again")
    }

    @Test
    fun `it reads from the profile, defaults filled`() {
        val care = ScreenCare.read(Json.parseToJsonElement("""{"shift":true,"dim":true,"dimFrom":1320,"dimTo":420}""") as JsonObject)
        assertEquals(ScreenCare(shift = true, dim = true, dimFrom = 1320, dimTo = 420, dimLevel = 10), care)
        assertEquals(ScreenCare(), ScreenCare.read(null))
    }
}

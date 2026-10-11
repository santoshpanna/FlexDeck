package dev.flexdeck.client

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class PagerTest {

    @Test
    fun `a wobble stays a press and a sideways move becomes a swipe`() {
        assertFalse(Pager.claims(dx = 6f, dy = 0f, slop = 16f), "inside the slop")
        assertFalse(Pager.claims(dx = 30f, dy = 28f, slop = 16f), "as much down as across")
        assertTrue(Pager.claims(dx = -30f, dy = 8f, slop = 16f))
    }

    @Test
    fun `past the first or last page the drag only partly shows`() {
        assertEquals(100f, Pager.offset(100f, index = 1, count = 3))
        assertEquals(30f, Pager.offset(100f, index = 0, count = 3), 0.001f)
        assertEquals(-30f, Pager.offset(-100f, index = 2, count = 3), 0.001f)
        assertEquals(-100f, Pager.offset(-100f, index = 0, count = 3))
    }

    @Test
    fun `a long drag or a flick turns the page and anything less springs back`() {
        // 1000 wide: 180 px is the line.
        assertEquals(1, Pager.settle(dx = -200f, velocity = 0f, width = 1000f, index = 0, count = 3))
        assertEquals(0, Pager.settle(dx = -100f, velocity = 0f, width = 1000f, index = 0, count = 3))
        assertEquals(1, Pager.settle(dx = -60f, velocity = -1500f, width = 1000f, index = 0, count = 3))
        assertEquals(1, Pager.settle(dx = 200f, velocity = 0f, width = 1000f, index = 2, count = 3))
    }

    @Test
    fun `a flick against the drag does not turn the page`() {
        assertEquals(1, Pager.settle(dx = -60f, velocity = 1500f, width = 1000f, index = 1, count = 3))
    }

    @Test
    fun `the ends hold and a single page never moves`() {
        assertEquals(0, Pager.settle(dx = 400f, velocity = 2000f, width = 1000f, index = 0, count = 3))
        assertEquals(2, Pager.settle(dx = -400f, velocity = -2000f, width = 1000f, index = 2, count = 3))
        assertEquals(0, Pager.settle(dx = -400f, velocity = -2000f, width = 1000f, index = 0, count = 1))
    }

    @Test
    fun `every page of the first profile is read, in order`() {
        val deck = DeckReader.read(
            Json.parseToJsonElement(
                """
                {"profiles":[{"id":"p","grid":{"cols":2,"rows":1,"usableW":200,"usableH":100},
                  "pages":[
                    {"id":"home","name":"Home","widgets":[{"id":"a","w":1,"h":1}]},
                    {"name":"Stream","widgets":[{"id":"b","w":1,"h":1},{"id":"c","col":1,"w":1,"h":1}]}
                  ]}]}
                """.trimIndent(),
            ) as JsonObject,
        )!!

        assertEquals(listOf("home", "page-2"), deck.pages.map { it.id })
        assertEquals("Stream", deck.pages[1].name)
        assertEquals(listOf("b", "c"), deck.pages[1].widgets.map { it.id })
        assertEquals(listOf("a"), deck.widgets.map { it.id }, "widgets is still the first page")
        assertEquals(1, deck.indexOf("page-2"))
        assertNull(deck.indexOf("gone"))
    }

    @Test
    fun `a profile without pages reads as one empty page`() {
        val deck = DeckReader.read(
            Json.parseToJsonElement("""{"profiles":[{"id":"p"}]}""") as JsonObject,
        )!!
        assertEquals(1, deck.pages.size)
        assertTrue(deck.widgets.isEmpty())
    }
}

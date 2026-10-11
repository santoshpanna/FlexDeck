package dev.flexdeck.client

import dev.flexdeck.grid.WidgetSpan
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class DeckPortraitTest {

    private fun read(text: String) = DeckReader.read(Json.parseToJsonElement(text) as JsonObject)!!

    @Test
    fun `portrait's own grid and a pinned place are read`() {
        val deck = read(
            """
            {"profiles":[{"id":"p",
              "grid":{"cols":6,"rows":4,"usableW":1280,"usableH":800},
              "portraitGrid":{"cols":3,"rows":9,"gapX":4},
              "pages":[{"id":"g","widgets":[
                {"id":"a","col":1,"w":1,"h":1,"portrait":{"col":2,"row":5,"w":1,"h":1}},
                {"id":"b","col":2,"w":1,"h":1}
              ]}]}]}
            """.trimIndent(),
        )
        val portrait = deck.portraitGrid!!
        assertEquals(3, portrait.cols)
        assertEquals(9, portrait.rows)
        assertEquals(4, portrait.gapX)
        // The panel's pixels are the device's: landscape's, turned.
        assertEquals(800, portrait.usableW)
        assertEquals(1280, portrait.usableH)
        assertEquals(WidgetSpan("a", 2, 5, 1, 1), deck.widgets[0].portrait)
        assertNull(deck.widgets[1].portrait)
    }

    @Test
    fun `without them, portrait is assumed`() {
        val deck = read(
            """{"profiles":[{"id":"p","grid":{"cols":6,"rows":4},"pages":[{"id":"g","widgets":[{"id":"a","w":1,"h":1}]}]}]}""",
        )
        assertNull(deck.portraitGrid)
        assertNull(deck.widgets.single().portrait)
    }
}

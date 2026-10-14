package dev.flexdeck.client

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class DeckTest {

    private fun config(text: String): JsonObject = Json.parseToJsonElement(text) as JsonObject

    @Test
    fun `a configuration reads into a deck`() {
        val deck = DeckReader.read(
            config(
                """
                {"schema":1,"rev":1,"profiles":[{
                  "id":"tab1",
                  "grid":{"cols":4,"rows":3,"gapX":8,"gapY":8,"usableW":800,"usableH":600},
                  "pages":[{"id":"home","widgets":[
                    {"id":"mute","type":"WIDGET_TYPE_BUTTON","col":1,"row":2,"w":2,"h":1,
                     "props":{"label":"Mute","bgColor":"#1e40af"}}
                  ]}]
                }]}
                """.trimIndent(),
            ),
        )!!

        assertEquals("tab1", deck.profileId)
        assertEquals(4, deck.grid.cols)
        assertEquals(800, deck.grid.usableW)
        assertEquals(1, deck.widgets.size)

        val widget = deck.widgets.single()
        assertEquals("mute", widget.id)
        assertEquals("Mute", widget.label)
        assertEquals("#1e40af", widget.background)
        assertEquals(1, widget.span.col)
        assertEquals(2, widget.span.row)
        assertEquals(false, widget.hideDot, "a lit key shows its dot unless told not to")
        assertEquals(2, widget.span.w)
    }

    @Test
    fun `a key can hide its corner dot`() {
        val deck = DeckReader.read(
            config(
                """
                {"schema":5,"rev":1,"profiles":[{"id":"tab1",
                  "grid":{"cols":4,"rows":3,"gapX":8,"gapY":8,"usableW":800,"usableH":600},
                  "pages":[{"id":"home","widgets":[
                    {"id":"scene","type":"WIDGET_TYPE_BUTTON","col":0,"row":0,"w":1,"h":1,"props":{"label":"Live","hideDot":true}}
                  ]}]
                }]}
                """.trimIndent(),
            ),
        )!!
        assertEquals(true, deck.widgets.single().hideDot)
    }

    @Test
    fun `a widget's looks are read by id`() {
        val deck = DeckReader.read(
            Json.parseToJsonElement(
                """
                {"profiles":[{"id":"tab1","pages":[{"id":"home","widgets":[
                  {"id":"rec","props":{"label":"Record"},
                   "states":{"looks":[{"id":"idle"},{"id":"recording","label":"REC","bgColor":"#b91c1c"},{"label":"no id"}],
                             "variable":{"variable":"obs.recording"}}}
                ]}]}]}
                """.trimIndent(),
            ) as JsonObject,
        )!!

        val looks = deck.widgets.single().looks
        assertEquals(setOf("idle", "recording"), looks.keys, "a look without an id is skipped")
        assertEquals(Look(label = "REC", background = "#b91c1c", foreground = null), looks["recording"])
        assertEquals(Look(label = null, background = null, foreground = null), looks["idle"])
    }

    @Test
    fun `colours parse to opaque ARGB, and anything else is null`() {
        assertEquals(0xFFB91C1C.toInt(), Colors.parse("#b91c1c"))
        assertEquals(0xFF000000.toInt(), Colors.parse("000000"))
        assertNull(Colors.parse("#fff"))
        assertNull(Colors.parse("red"))
        assertNull(Colors.parse(null))
    }

    @Test
    fun `absent grid fields are zero, not invented`() {
        // The proto3 JSON mapping omits defaults, so a grid with no gaps sends no
        // gap fields at all. Treating absence as "unset, pick something sensible"
        // would silently add gaps a user had deliberately removed — and the
        // tablet would disagree with the preview.
        val deck = DeckReader.read(
            config("""{"profiles":[{"id":"p","grid":{"cols":4,"rows":3},"pages":[{"id":"home"}]}]}"""),
        )!!

        assertEquals(0, deck.grid.gapX)
        assertEquals(0, deck.grid.gapY)
        assertEquals(0, deck.grid.usableW)
    }

    @Test
    fun `a widget with no span still occupies a cell`() {
        // A zero-sized widget would compute a zero-width rectangle and draw
        // nothing, which reads as a broken renderer rather than a missing field.
        val deck = DeckReader.read(
            config("""{"profiles":[{"id":"p","grid":{"cols":2,"rows":2},"pages":[{"id":"h","widgets":[{"id":"w"}]}]}]}"""),
        )!!

        val widget = deck.widgets.single()
        assertEquals(1, widget.span.w)
        assertEquals(1, widget.span.h)
        assertEquals("w", widget.label, "an unlabelled widget shows its id")
    }

    @Test
    fun `a configuration with no profiles is no deck`() {
        assertNull(DeckReader.read(config("""{"schema":1,"rev":1}""")))
        assertNull(DeckReader.read(null))
    }

    @Test
    fun `a profile with no pages still yields a deck`() {
        // The grid is known even when there is nothing to draw on it, and showing
        // an empty grid is more useful than showing nothing.
        val deck = DeckReader.read(config("""{"profiles":[{"id":"p","grid":{"cols":3,"rows":3}}]}"""))!!

        assertEquals(3, deck.grid.cols)
        assertEquals(0, deck.widgets.size)
    }

    @Test
    fun `labels default to the id only when they are blank`() {
        val deck = DeckReader.read(
            config(
                """{"profiles":[{"id":"p","grid":{"cols":2,"rows":2},"pages":[{"id":"h","widgets":[
                   {"id":"a","props":{"label":"   "}},
                   {"id":"b","props":{"label":"Real"}}
                ]}]}]}""",
            ),
        )!!

        assertEquals("a", deck.widgets[0].label)
        assertEquals("Real", deck.widgets[1].label)
    }

    @Test
    fun `absent colours are null rather than empty strings`() {
        val deck = DeckReader.read(
            config("""{"profiles":[{"id":"p","grid":{"cols":1,"rows":1},"pages":[{"id":"h","widgets":[{"id":"a"}]}]}]}"""),
        )!!

        assertNull(deck.widgets.single().background)
        assertNull(deck.widgets.single().foreground)
    }
}

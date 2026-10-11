package dev.flexdeck.client

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class SliderTest {

    private val percent = SliderSpec(min = 0.0, max = 100.0, step = 1.0)

    @Test
    fun `auto follows the longer side, and an explicit direction wins`() {
        assertTrue(SliderMath.isVertical(percent, width = 100f, height = 300f))
        assertFalse(SliderMath.isVertical(percent, width = 300f, height = 100f))
        val horizontal = percent.copy(orientation = SliderOrientation.HORIZONTAL)
        assertFalse(SliderMath.isVertical(horizontal, width = 100f, height = 300f))
    }

    @Test
    fun `a vertical slider's bottom is its minimum, like a fader`() {
        assertEquals(0.0, SliderMath.valueAt(percent, 100f, 300f, x = 50f, y = 300f))
        assertEquals(100.0, SliderMath.valueAt(percent, 100f, 300f, x = 50f, y = 0f))
        assertEquals(50.0, SliderMath.valueAt(percent, 100f, 300f, x = 50f, y = 150f))
    }

    @Test
    fun `a finger past either end holds the end, rather than jumping`() {
        assertEquals(100.0, SliderMath.valueAt(percent, 300f, 100f, x = 900f, y = 50f))
        assertEquals(0.0, SliderMath.valueAt(percent, 300f, 100f, x = -40f, y = 50f))
    }

    @Test
    fun `a drag moves the value by how far the finger went, never to where it landed`() {
        // Landing changes nothing, wherever on the tile it is.
        assertEquals(40.0, SliderMath.dragValue(percent, vertical = true, 40.0, startPos = 10f, pos = 10f, length = 200f))
        // Up a quarter of the track: a quarter of the range louder.
        assertEquals(65.0, SliderMath.dragValue(percent, vertical = true, 40.0, startPos = 150f, pos = 100f, length = 200f))
        // Across: right is louder; past the end holds the end.
        assertEquals(15.0, SliderMath.dragValue(percent, vertical = false, 40.0, startPos = 100f, pos = 50f, length = 200f))
        assertEquals(100.0, SliderMath.dragValue(percent, vertical = false, 40.0, startPos = 0f, pos = 900f, length = 200f))
        assertEquals(40.0, SliderMath.dragValue(percent, vertical = true, 40.0, startPos = 0f, pos = 90f, length = 0f))
    }

    @Test
    fun `values snap to the step and stay in range`() {
        val coarse = SliderSpec(min = 0.0, max = 10.0, step = 2.5)
        assertEquals(2.5, SliderMath.settle(coarse, 3.7))
        assertEquals(10.0, SliderMath.settle(coarse, 99.0))
        assertEquals(0.0, SliderMath.settle(coarse, Double.NaN))
        assertEquals(0.25f, SliderMath.fraction(coarse, 2.5))
        assertEquals(37.3, SliderMath.settle(SliderSpec(), 37.3), "no step: no snapping")
    }

    @Test
    fun `the pacer sends at most one value per interval, the newest, and always the last`() {
        val pacer = ValuePacer(intervalMs = 33)
        assertEquals(10.0, pacer.move(10.0, nowMs = 0))
        assertNull(pacer.move(11.0, nowMs = 5), "too soon")
        assertNull(pacer.move(12.0, nowMs = 10), "still too soon; 12 replaces 11")
        assertEquals(23L, pacer.dueInMs(nowMs = 10))
        assertNull(pacer.flush(nowMs = 20), "not due yet")
        assertEquals(12.0, pacer.flush(nowMs = 33), "the newest held-back value")
        assertNull(pacer.dueInMs(nowMs = 33), "nothing left")
        assertNull(pacer.move(12.0, nowMs = 100), "an unchanged value is not sent again")
        assertEquals(12.0, pacer.release(12.0, nowMs = 101), "the lift is always sent")
        assertEquals(12.0, pacer.move(12.0, nowMs = 200), "a new drag starts fresh")
    }

    @Test
    fun `a slider widget reads its range and direction, and defaults like the agent`() {
        val deck = DeckReader.read(
            Json.parseToJsonElement(
                """
                {"profiles":[{"id":"p","pages":[{"id":"home","widgets":[
                  {"id":"vol","type":"WIDGET_TYPE_SLIDER","h":3,
                   "slider":{"orientation":"SLIDER_ORIENTATION_VERTICAL","step":5}},
                  {"id":"ptt","flows":[{"trigger":{"gesture":"GESTURE_HOLD_START"}},
                                       {"trigger":{"gesture":"GESTURE_HOLD_END"}}]},
                  {"id":"knob","type":"WIDGET_TYPE_DIAL"}
                ]}]}]}
                """.trimIndent(),
            ) as JsonObject,
        )!!
        val (vol, ptt, knob) = deck.widgets
        assertEquals(WidgetKind.SLIDER, vol.kind)
        assertEquals(SliderSpec(0.0, 100.0, 5.0, SliderOrientation.VERTICAL), vol.slider)
        assertEquals(WidgetKind.BUTTON, ptt.kind, "no type: a button")
        assertTrue(ptt.holds)
        assertFalse(vol.holds)
        assertEquals(WidgetKind.UNKNOWN, knob.kind, "a newer agent's widget is not a crash")
        assertNull(knob.slider)
    }

    @Test
    fun `a state's value is absent, zero, or a number — and absent keeps the last one`() {
        val zero = Frames.parseServerMessage("""{"state":{"widgetId":"vol","ok":true,"value":0}}""")
        assertEquals(0.0, (zero as ServerMessage.State).value)
        val none = Frames.parseServerMessage("""{"state":{"widgetId":"vol","ok":true}}""")
        assertNull((none as ServerMessage.State).value)

        val session = DeckSession()
        session.apply(ServerMessage.State(widgetId = "vol", value = 40.0))
        session.apply(ServerMessage.State(widgetId = "vol", ok = false, error = "OBS is not running"))
        assertEquals(40.0, session.liveWidgets["vol"]?.value, "a failure report does not move the thumb")
    }
}

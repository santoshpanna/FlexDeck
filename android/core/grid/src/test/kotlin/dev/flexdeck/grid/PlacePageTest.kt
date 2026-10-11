package dev.flexdeck.grid

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** The same cases as packages/grid/test/place.test.ts: the editor and the tablet must agree. */
class PlacePageTest {
    private val landscape = GridSpec(cols = 6, rows = 4, gapX = 8, gapY = 8, usableW = 1280, usableH = 800)
    private val portrait = GridSpec(cols = 3, rows = 9, gapX = 8, gapY = 8, usableW = 800, usableH = 1280)

    private fun at(id: String, col: Int, row: Int, w: Int = 1, h: Int = 1) = WidgetSpan(id, col, row, w, h)

    private fun spans(page: PlacedPage) = page.widgets.map { it.span to it.assumed }

    @Test
    fun `landscape is the authored layout`() {
        val page = GridLayout.placePage(landscape, portrait, listOf(PlacedWidget(at("a", 5, 3))), Orientation.LANDSCAPE)
        assertEquals(listOf(at("a", 5, 3) to false), spans(page))
    }

    @Test
    fun `no portrait grid and nothing pinned is the old transpose`() {
        val widgets = listOf(PlacedWidget(at("a", 2, 1, 2, 1)), PlacedWidget(at("b", 5, 3)))
        val placed = GridLayout.placePage(landscape, null, widgets, Orientation.PORTRAIT)
        val old = GridLayout.orientPage(landscape, widgets.map { it.span }, Orientation.PORTRAIT)
        assertEquals(old.spec, placed.spec)
        assertEquals(old.widgets, placed.widgets.map { it.span })
        assertTrue(placed.widgets.all { it.assumed })
    }

    @Test
    fun `a turned span that fits its own portrait grid stays where turning puts it`() {
        val page = GridLayout.placePage(landscape, portrait, listOf(PlacedWidget(at("a", 5, 2))), Orientation.PORTRAIT)
        assertEquals(listOf(at("a", 2, 5) to true), spans(page))
    }

    @Test
    fun `a turned span that falls outside the portrait grid takes the first free cell`() {
        val widgets = listOf(PlacedWidget(at("x", 0, 0)), PlacedWidget(at("a", 0, 3)))
        val page = GridLayout.placePage(landscape, portrait, widgets, Orientation.PORTRAIT)
        assertEquals(listOf(at("x", 0, 0) to true, at("a", 1, 0) to true), spans(page))
    }

    @Test
    fun `a pinned position is kept, and assumed widgets go around it`() {
        val widgets = listOf(PlacedWidget(at("a", 0, 0)), PlacedWidget(at("b", 1, 0), portrait = at("b", 0, 0)))
        val page = GridLayout.placePage(landscape, portrait, widgets, Orientation.PORTRAIT)
        assertEquals(listOf(at("a", 1, 0) to true, at("b", 0, 0) to false), spans(page))
    }

    @Test
    fun `a pin that no longer fits the grid falls back to an assumed position`() {
        val widgets = listOf(PlacedWidget(at("a", 0, 0), portrait = at("a", 7, 7)))
        val page = GridLayout.placePage(landscape, portrait, widgets, Orientation.PORTRAIT)
        assertEquals(listOf(at("a", 0, 0) to true), spans(page))
    }

    @Test
    fun `a span too wide for the portrait grid is narrowed to fit`() {
        val page = GridLayout.placePage(landscape, portrait.copy(cols = 2, rows = 2), listOf(PlacedWidget(at("a", 0, 0, 1, 4))), Orientation.PORTRAIT)
        assertEquals(listOf(at("a", 0, 0, 2, 1) to true), spans(page))
    }

    @Test
    fun `a widget that fits nowhere is left out`() {
        val page = GridLayout.placePage(landscape, portrait.copy(cols = 1, rows = 1), listOf(PlacedWidget(at("a", 0, 0)), PlacedWidget(at("b", 1, 0))), Orientation.PORTRAIT)
        assertEquals(listOf("a"), page.widgets.map { it.span.id })
    }
}

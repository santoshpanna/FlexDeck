package dev.flexdeck.client

import dev.flexdeck.grid.GridSpec
import dev.flexdeck.grid.WidgetSpan
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

/**
 * A configuration, read the way a renderer needs it.
 *
 * Every read here is `?: default`, and that is load-bearing rather than
 * defensive: the agent speaks the **proto3 JSON mapping**, which omits default
 * values. An absent gap means a gap of *zero*, not "unset" — inventing a
 * different default would silently change the layout for a user who chose no
 * gaps, and the tablet would disagree with the preview.
 *
 * Spans are clamped up to one cell because the grid algorithm cannot express a
 * zero-sized widget: it would compute a zero-width rectangle and draw nothing at
 * all, which reads as a rendering bug rather than a gap in the configuration.
 */
data class Deck(
    val profileId: String,
    /** The landscape grid: what widgets' own spans are placed on. */
    val grid: GridSpec,
    /** Every page, in order. Never empty: a profile without pages reads as one empty page. */
    val pages: List<DeckPage>,
    /** Portrait's own grid, or `null` for the landscape one turned on its side. */
    val portraitGrid: GridSpec? = null,
    /** Burn-in shift and night dimming. */
    val screen: ScreenCare = ScreenCare(),
) {
    /** The first page's widgets: what a deck with one page shows. */
    val widgets: List<DeckWidget> get() = pages.first().widgets

    /** Which page `pageId` is, or `null` when this deck has no such page any more. */
    fun indexOf(pageId: String?): Int? =
        pages.indexOfFirst { it.id == pageId }.takeIf { it >= 0 }
}

/** One page of a profile: the grid is the profile's, the widgets its own. */
data class DeckPage(
    val id: String,
    val name: String,
    val widgets: List<DeckWidget>,
    /** Behind every key: a picture or a gradient across the whole page. */
    val background: BackgroundSpec = BackgroundSpec(),
)

data class DeckWidget(
    val id: String,
    val label: String,
    val span: WidgetSpan,
    val background: String?,
    val foreground: String?,
    /** The widget's looks by id — Muted, Live — for a multi-state widget. */
    val looks: Map<String, Look> = emptyMap(),
    val kind: WidgetKind = WidgetKind.BUTTON,
    /** Set for a slider: its range and direction. */
    val slider: SliderSpec? = null,
    /**
     * Whether it has flows for a finger going down and lifting. Such a button
     * reports the hold — push-to-talk — instead of an instant press.
     */
    val holds: Boolean = false,
    /** A bundled icon's name or `img:<sha256>`; `null` for none. */
    val icon: String? = null,
    /**
     * Every gesture it has a flow for. Decides how a press is recognised: a key
     * with a double tap or a long press has to wait to see which one it is.
     */
    val gestures: Set<String> = emptySet(),
    /** Where it was put in portrait; `null` while portrait assumes its place (`GridLayout.placePage`). */
    val portrait: WidgetSpan? = null,
    /**
     * The face of each look, its own under "" — worked out once here, as the
     * agent and the editor work it out (see [Faces]).
     */
    val faces: Map<String, FaceSpec> = mapOf("" to FaceSpec()),
    /** Whether its own face is the one schema 3 drew from its props. */
    val derived: Boolean = true,
    /** `src.<source>.@<id>`: its own run of its first data source, which `this.` names. */
    val thisBase: String? = null,
    /** Buttons along its bottom strip, each pressed on its own. */
    val zones: List<ZoneSpec> = emptyList(),
    /** No light in the corner while it is lit; its look still changes. */
    val hideDot: Boolean = false,
) {
    /** A variable as the agent names it: `this.today` → `src.weather.@w12.today`. */
    fun resolve(name: String): String =
        if (name.startsWith("this.") && thisBase != null) "$thisBase.${name.removePrefix("this.")}" else name
}

/** What kind of widget it is. One this build does not know is drawn as a plain tile. */
enum class WidgetKind { BUTTON, SLIDER, UNKNOWN }

/** One look of a multi-state widget. Absent fields fall back to the widget's. */
data class Look(
    val label: String?,
    val background: String?,
    val foreground: String?,
    val icon: String? = null,
)

/** `#rrggbb` as an opaque ARGB int, or `null` when it is not one. */
object Colors {
    fun parse(hex: String?): Int? {
        val digits = hex?.trim()?.removePrefix("#") ?: return null
        if (digits.length != 6) return null
        val rgb = digits.toLongOrNull(16) ?: return null
        return (0xFF000000L or rgb).toInt()
    }
}

object DeckReader {

    /** Read the first profile of a configuration, every page of it. `null` when there is none. */
    fun read(config: JsonObject?): Deck? {
        val profile = config?.array("profiles")?.firstObjectOrNull() ?: return null
        val pages = (profile.array("pages") ?: JsonArray(emptyList()))
            .mapNotNull { it as? JsonObject }
            .mapIndexed { index, page ->
                DeckPage(
                    // Positional when absent, so two unnamed pages stay apart.
                    id = page.string("id").ifBlank { "page-${index + 1}" },
                    name = page.string("name"),
                    widgets = (page.array("widgets") ?: JsonArray(emptyList()))
                        .mapNotNull { it as? JsonObject }
                        .map(::readWidget),
                    background = (page["background"] as? JsonObject)
                        ?.let { FaceReader.read(JsonObject(mapOf("background" to it))).background }
                        ?: BackgroundSpec(),
                )
            }
            .ifEmpty { listOf(DeckPage(id = "page-1", name = "", widgets = emptyList())) }

        return Deck(
            profileId = profile.string("id"),
            grid = readGrid(profile["grid"] as? JsonObject),
            pages = pages,
            screen = ScreenCare.read(profile["screen"] as? JsonObject),
            portraitGrid = (profile["portraitGrid"] as? JsonObject)?.let { own ->
                // The panel's pixels are the device's, so only the cells and gaps are portrait's own.
                val landscape = readGrid(profile["grid"] as? JsonObject)
                readGrid(own).copy(usableW = landscape.usableH, usableH = landscape.usableW)
            },
        )
    }

    private fun readGrid(grid: JsonObject?): GridSpec = GridSpec(
        cols = grid.int("cols"),
        rows = grid.int("rows"),
        gapX = grid.int("gapX"),
        gapY = grid.int("gapY"),
        usableW = grid.int("usableW"),
        usableH = grid.int("usableH"),
    )

    internal fun readWidget(widget: JsonObject): DeckWidget {
        val props = widget["props"] as? JsonObject
        // Proto3 JSON omits the default, and the default is UNSPECIFIED, which
        // the agent treats as a button.
        val kind = when (widget.string("type")) {
            "", "WIDGET_TYPE_UNSPECIFIED", "WIDGET_TYPE_BUTTON" -> WidgetKind.BUTTON
            "WIDGET_TYPE_SLIDER" -> WidgetKind.SLIDER
            else -> WidgetKind.UNKNOWN
        }
        // A zone's flows are the zone's: they never make the key wait for a long press.
        val gestures = (widget.array("flows") ?: JsonArray(emptyList()))
            .filter { (it as? JsonObject)?.string("zone").isNullOrBlank() }
            .mapNotNull { ((it as? JsonObject)?.get("trigger") as? JsonObject)?.string("gesture") }

        return DeckWidget(
            id = widget.string("id"),
            // Falling back to the id matters: an unlabelled button showing
            // nothing at all is indistinguishable from a rendering failure.
            label = props.string("label").ifBlank { widget.string("id") },
            span = WidgetSpan(
                id = widget.string("id"),
                col = widget.int("col"),
                row = widget.int("row"),
                w = maxOf(1, widget.int("w")),
                h = maxOf(1, widget.int("h")),
            ),
            background = props.string("bgColor").ifBlank { null },
            foreground = props.string("fgColor").ifBlank { null },
            looks = readLooks(widget["states"] as? JsonObject),
            kind = kind,
            slider = if (kind == WidgetKind.SLIDER) readSlider(widget["slider"] as? JsonObject) else null,
            holds = gestures.any { it == "GESTURE_HOLD_START" || it == "GESTURE_HOLD_END" },
            icon = props.string("icon").ifBlank { null },
            gestures = gestures.toSet(),
            faces = buildMap {
                put("", FaceReader.read(Faces.faceFor(widget, null)))
                for (look in ((widget["states"] as? JsonObject)?.array("looks") ?: JsonArray(emptyList()))) {
                    val id = (look as? JsonObject)?.string("id")?.takeIf { it.isNotBlank() } ?: continue
                    put(id, FaceReader.read(Faces.faceFor(widget, id)))
                }
            },
            derived = widget["face"] == null,
            hideDot = props.string("hideDot") == "true",
            zones = FaceReader.zones(widget["face"] as? JsonObject),
            thisBase = ((widget["sources"] as? JsonArray)?.firstOrNull() as? JsonObject)
                ?.string("source")?.takeIf { it.isNotBlank() }
                ?.let { "src.$it.@${widget.string("id")}" },
            portrait = (widget["portrait"] as? JsonObject)?.let { placed ->
                WidgetSpan(
                    id = widget.string("id"),
                    col = placed.int("col"),
                    row = placed.int("row"),
                    w = maxOf(1, placed.int("w")),
                    h = maxOf(1, placed.int("h")),
                )
            },
        )
    }

    private fun readSlider(slider: JsonObject?): SliderSpec {
        val min = slider.double("min")
        val max = slider.double("max")
        // Both absent: the agent's 0-100 default, so the two agree.
        val (low, high) = if (min == 0.0 && max == 0.0) 0.0 to 100.0 else min to max
        return SliderSpec(
            min = low,
            max = if (high > low) high else low + 1.0,
            step = slider.double("step").coerceAtLeast(0.0),
            orientation = when (slider.string("orientation")) {
                "SLIDER_ORIENTATION_HORIZONTAL" -> SliderOrientation.HORIZONTAL
                "SLIDER_ORIENTATION_VERTICAL" -> SliderOrientation.VERTICAL
                else -> SliderOrientation.AUTO
            },
        )
    }

    private fun readLooks(states: JsonObject?): Map<String, Look> =
        (states?.array("looks") ?: JsonArray(emptyList()))
            .mapNotNull { it as? JsonObject }
            .filter { it.string("id").isNotBlank() }
            .associate { look ->
                look.string("id") to Look(
                    label = look.string("label").ifBlank { null },
                    background = look.string("bgColor").ifBlank { null },
                    foreground = look.string("fgColor").ifBlank { null },
                    icon = look.string("icon").ifBlank { null },
                )
            }
}

private fun JsonObject?.int(key: String): Int =
    (this?.get(key) as? JsonPrimitive)?.content?.toIntOrNull() ?: 0

private fun JsonObject?.double(key: String): Double =
    (this?.get(key) as? JsonPrimitive)?.content?.toDoubleOrNull() ?: 0.0

private fun JsonObject?.string(key: String): String =
    (this?.get(key) as? JsonPrimitive)?.content ?: ""

private fun JsonObject.array(key: String): JsonArray? = this[key] as? JsonArray

private fun JsonArray.firstObjectOrNull(): JsonObject? = firstOrNull() as? JsonObject

package dev.flexdeck.client

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

/**
 * The face a widget shows in a look, worked out as the agent and the editor
 * work it out (`faces.rs`, `packages/face`), held together by
 * `contract/fixtures/face/derive.json`.
 *
 * Done on the JSON, as the wire carries it, so the fixture compares exactly;
 * [FaceReader] then turns the result into what drawing needs.
 */
object Faces {

    /** The face [widget] shows in look [lookId]; absent or unknown: its own. */
    fun faceFor(widget: JsonObject, lookId: String?): JsonObject {
        val own = widget["face"] as? JsonObject
        val props = widget["props"] as? JsonObject
        val look = lookId?.let { id ->
            ((widget["states"] as? JsonObject)?.get("looks") as? JsonArray)
                ?.mapNotNull { it as? JsonObject }
                ?.firstOrNull { it.text("id") == id }
        } ?: return own ?: derive(props)

        (look["face"] as? JsonObject)?.let { return it }

        if (own == null) {
            val merged = props.orEmpty().toMutableMap()
            for ((from, to) in listOf("label" to "label", "icon" to "icon", "bgColor" to "bgColor", "fgColor" to "fgColor")) {
                if (look.text(from).isNotBlank()) merged[to] = look[from]!!
            }
            return derive(JsonObject(merged))
        }

        val elements = (own["elements"] as? JsonArray).orEmpty().map { (it as? JsonObject).orEmpty().toMutableMap() }
        if (look.text("label").isNotBlank()) {
            elements.firstOrNull { it.text("slot", "SLOT_TITLE") == "SLOT_TITLE" && it.text("kind", "ELEMENT_KIND_TEXT") == "ELEMENT_KIND_TEXT" }
                ?.set("template", look["label"]!!)
        }
        if (look.text("icon").isNotBlank()) {
            elements.firstOrNull { it.text("kind") == "ELEMENT_KIND_ICON" }?.set("template", look["icon"]!!)
        }
        if (look.text("fgColor").isNotBlank()) {
            for (element in elements) {
                val kind = element.text("kind", "ELEMENT_KIND_TEXT")
                if (kind == "ELEMENT_KIND_TEXT" || kind == "ELEMENT_KIND_ICON") element["color"] = look["fgColor"]!!
            }
        }
        val face = own.toMutableMap()
        if (elements.isNotEmpty()) face["elements"] = JsonArray(elements.map { JsonObject(it) })
        if (look.text("bgColor").isNotBlank()) {
            face["background"] = JsonObject((own["background"] as? JsonObject).orEmpty() + ("color" to look["bgColor"]!!))
        }
        return JsonObject(face)
    }

    /** The face schema 3 drew from `props`: an icon over a label. */
    fun derive(props: JsonObject?): JsonObject {
        val p = props.orEmpty()
        val elements = mutableListOf<JsonObject>()
        val icon = firstSet(p.text("iconTemplate"), p.text("icon"))
        val color = p.text("fgColor").trim()
        if (icon.isNotEmpty()) {
            elements += compact(
                "slot" to "SLOT_ICON",
                "kind" to "ELEMENT_KIND_ICON",
                "template" to icon,
                "color" to color,
            )
        }
        elements += compact("template" to firstSet(p.text("labelTemplate"), p.text("label")), "color" to color)
        val face = mutableMapOf<String, JsonElement>()
        val background = p.text("bgColor").trim()
        if (background.isNotEmpty()) face["background"] = JsonObject(mapOf("color" to JsonPrimitive(background)))
        face["elements"] = JsonArray(elements)
        return JsonObject(face)
    }

    private fun firstSet(vararg candidates: String): String =
        candidates.map { it.trim() }.firstOrNull { it.isNotEmpty() } ?: ""

    /** Only the fields that are set, as the proto3 JSON mapping writes them. */
    private fun compact(vararg fields: Pair<String, String>): JsonObject =
        JsonObject(fields.filter { it.second.isNotEmpty() }.associate { it.first to JsonPrimitive(it.second) })

    private fun JsonObject?.orEmpty(): JsonObject = this ?: JsonObject(emptyMap())

    private fun Map<String, JsonElement>.text(key: String, default: String = ""): String =
        (this[key] as? JsonPrimitive)?.content ?: default
}

/** A face, as drawing needs it. */
/** One button of a face's bottom strip: its name, its icon, and the icon while its `when` holds. */
data class ZoneSpec(val name: String, val icon: String = "", val iconOn: String = "")

data class FaceSpec(
    val layout: String = "FACE_LAYOUT_ICON_LABEL",
    val background: BackgroundSpec = BackgroundSpec(),
    val elements: List<ElementSpec> = emptyList(),
)

data class BackgroundSpec(
    val color: String = "",
    val colorTo: String = "",
    /** The fade's direction, CSS degrees: 0 up, 90 right, 180 (the default) down. */
    val angle: Int = 180,
    /** A circle out from the middle rather than a line across. */
    val radial: Boolean = false,
    val image: String = "",
    val contain: Boolean = false,
    val dim: Int = 0,
    val graph: String = "",
    val graphColor: String = "",
    val transparent: Boolean = false,
)

data class ElementSpec(
    val slot: String = "SLOT_TITLE",
    val kind: String = "ELEMENT_KIND_TEXT",
    val template: String = "",
    val variable: String = "",
    val min: Double = 0.0,
    val max: Double = 0.0,
    val size: String = "TEXT_SIZE_AUTO",
    val bold: Boolean? = null,
    val align: String = "TEXT_ALIGN_AUTO",
    val font: String = "FONT_SANS",
    val maxLines: Int = 0,
    /** Too long: one line, scrolling sideways. */
    val marquee: Boolean = false,
    val color: String = "",
    val analog: Boolean = false,
    val seconds: Boolean = false,
    val format: String = "",
    val zone: String = "",
    val graphStyle: String = "GRAPH_STYLE_AREA",
    val points: Int = 0,
    /** Put anywhere: fractions of the face. `null`: its slot's place. */
    val box: Box? = null,
) {
    /** Whether its text comes from the agent: a template with holes. */
    val live: Boolean get() = template.contains("{{")
}

/**
 * Where a fade runs across a `width` x `height` face, as CSS draws it: the
 * line through the middle at `angle` (0 up, 90 right), long enough that its
 * ends meet the far corners. Answers x0, y0, x1, y1 from the face's top left.
 */
object Fade {
    fun line(width: Float, height: Float, angle: Int): FloatArray {
        val radians = Math.toRadians(angle.toDouble())
        val dx = Math.sin(radians).toFloat()
        val dy = -Math.cos(radians).toFloat()
        val half = (Math.abs(width * dx) + Math.abs(height * dy)) / 2f
        val cx = width / 2f
        val cy = height / 2f
        return floatArrayOf(cx - dx * half, cy - dy * half, cx + dx * half, cy + dy * half)
    }

    /** A round fade's radius: out to the far corners, as CSS's `circle` is. */
    fun radius(width: Float, height: Float): Float = Math.hypot(width / 2.0, height / 2.0).toFloat().coerceAtLeast(1f)
}

object FaceReader {
    /** The zones of a widget's own face, in order. */
    fun zones(face: JsonObject?): List<ZoneSpec> =
        (face?.get("zones") as? JsonArray).orEmpty().mapNotNull { zone ->
            val z = zone as? JsonObject ?: return@mapNotNull null
            ZoneSpec(name = z.text("name"), icon = z.text("icon"), iconOn = z.text("iconOn")).takeIf { it.name.isNotBlank() }
        }

    fun read(face: JsonObject): FaceSpec {
        val background = face["background"] as? JsonObject
        return FaceSpec(
            layout = face.text("layout").ifEmpty { "FACE_LAYOUT_ICON_LABEL" },
            background = BackgroundSpec(
                color = background.text("color"),
                colorTo = background.text("colorTo"),
                // 0 is unset on the wire, so straight up arrives as 360.
                angle = background.number("angle").toInt().let { if (it == 0) 180 else Math.floorMod(it, 360) },
                radial = background.text("gradient") == "GRADIENT_RADIAL",
                image = background.text("image"),
                contain = background.text("fit") == "IMAGE_FIT_CONTAIN",
                dim = background.number("dim").toInt().coerceIn(0, 90),
                graph = background.text("graph"),
                graphColor = background.text("graphColor"),
                transparent = background.text("transparent") == "true",
            ),
            elements = (face["elements"] as? JsonArray).orEmpty().mapNotNull { element ->
                val e = element as? JsonObject ?: return@mapNotNull null
                val text = e["text"] as? JsonObject
                val clock = e["clock"] as? JsonObject
                val graph = e["graph"] as? JsonObject
                ElementSpec(
                    slot = e.text("slot").ifEmpty { "SLOT_TITLE" },
                    kind = e.text("kind").ifEmpty { "ELEMENT_KIND_TEXT" },
                    template = e.text("template"),
                    variable = e.text("variable"),
                    min = e.number("min"),
                    max = e.number("max"),
                    size = text.text("size").ifEmpty { "TEXT_SIZE_AUTO" },
                    bold = text?.get("bold")?.let { (it as? JsonPrimitive)?.content == "true" },
                    align = text.text("align").ifEmpty { "TEXT_ALIGN_AUTO" },
                    font = text.text("font").ifEmpty { "FONT_SANS" },
                    maxLines = text.number("maxLines").toInt(),
                    marquee = text.text("marquee") == "true",
                    color = e.text("color"),
                    analog = clock.text("analog") == "true",
                    seconds = clock.text("seconds") == "true",
                    format = clock.text("format"),
                    zone = clock.text("zone"),
                    graphStyle = graph.text("style").ifEmpty { "GRAPH_STYLE_AREA" },
                    points = graph.number("points").toInt(),
                    box = (e["box"] as? JsonObject)?.let { b ->
                        Box(b.number("x"), b.number("y"), b.number("w"), b.number("h")).takeIf { it.w > 0 && it.h > 0 }
                    },
                )
            },
        )
    }

    private fun JsonObject?.text(key: String): String = (this?.get(key) as? JsonPrimitive)?.content ?: ""

    private fun JsonObject?.number(key: String): Double =
        (this?.get(key) as? JsonPrimitive)?.content?.toDoubleOrNull() ?: 0.0
}

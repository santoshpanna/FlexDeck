package dev.flexdeck.client

import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToLong

/** A place on a key, in the key's own pixels from its top-left. */
data class Box(val x: Double, val y: Double, val w: Double, val h: Double) {
    val right: Double get() = x + w
    val bottom: Double get() = y + h
}

/** How text in a place is drawn when its element does not say. */
data class PlaceStyle(
    val size: String,
    val align: String,
    val bold: Boolean,
    val maxLines: Int,
    /** Drawn at reduced strength: a caption, a top line. */
    val quiet: Boolean,
)

/**
 * Where each place of a layout falls on a key: the tablet's copy of
 * `placeFace` in `packages/face`, held to it by
 * `contract/fixtures/face/layout.expected.json`.
 */
object FaceLayout {
    val SLOTS = listOf("SLOT_TITLE", "SLOT_ICON", "SLOT_VALUE", "SLOT_CAPTION", "SLOT_TOP", "SLOT_BADGE", "SLOT_VISUAL")

    /** Text sizes at unit 1, in pixels. */
    fun textPx(size: String): Double = when (size) {
        "TEXT_SIZE_XS" -> 10.0
        "TEXT_SIZE_S" -> 12.0
        "TEXT_SIZE_L" -> 22.0
        "TEXT_SIZE_XL" -> 32.0
        else -> 15.0
    }

    private fun band(px: Double) = px * 1.3

    private fun r2(n: Double) = (n * 100).roundToLong() / 100.0

    private fun box(x: Double, y: Double, w: Double, h: Double) = Box(r2(x), r2(y), r2(max(0.0, w)), r2(max(0.0, h)))

    private fun square(area: Box, scale: Double): Box {
        val side = min(area.w, area.h) * scale
        return box(area.x + (area.w - side) / 2, area.y + (area.h - side) / 2, side, side)
    }

    /** A face's bottom strip of zones, and the room left above it for the rest. */
    data class Zones(val face: Box, val zones: List<Box>)

    /** `count` equal zones along the bottom of a key `w` × `h`; none: the face has the whole key. */
    fun placeZones(count: Int, w: Double, h: Double, unit: Double): Zones {
        if (count <= 0) return Zones(box(0.0, 0.0, w, h), emptyList())
        val strip = min(h * 0.5, max(30 * unit, h * 0.3))
        val top = h - strip
        return Zones(box(0.0, 0.0, w, top), (0 until count).map { box(w * it / count, top, w / count, strip) })
    }

    /** A freely placed element's box (fractions) on a face `w` × `h`, in pixels. */
    fun placeBox(box: Box, w: Double, h: Double): Box = box(box.x * w, box.y * h, box.w * w, box.h * h)

    fun place(layout: String, w: Double, h: Double, unit: Double): Map<String, Box> {
        val pad = min(w, h) * 0.08
        val x = pad
        val y = pad
        val width = w - pad * 2
        val height = h - pad * 2
        val m = min(width, height)
        val gap = m * 0.04
        val xs = band(textPx("TEXT_SIZE_XS") * unit)
        val s = band(textPx("TEXT_SIZE_S") * unit)
        val md = band(textPx("TEXT_SIZE_M") * unit)
        val badgeH = xs * 1.1
        val badge = box(x + width - badgeH * 2.2, y, badgeH * 2.2, badgeH)
        val top = box(x, y, width, xs)
        val whole = box(x, y, width, height)

        return when (layout) {
            "FACE_LAYOUT_VALUE" -> {
                val head = box(x, y, width, s)
                val caption = box(x, y + height - s, width, s)
                val value = box(x, head.y + head.h, width, caption.y - (head.y + head.h))
                mapOf(
                    "SLOT_TITLE" to head, "SLOT_TOP" to head, "SLOT_ICON" to box(x, y, s, s),
                    "SLOT_VALUE" to value, "SLOT_CAPTION" to caption, "SLOT_BADGE" to badge, "SLOT_VISUAL" to whole,
                )
            }
            "FACE_LAYOUT_ART" -> {
                val caption = box(x, y + height - s, width, s)
                val title = box(x, caption.y - md, width, md)
                val strip = max(4 * unit, m * 0.05)
                val visual = box(x, title.y - gap - strip, width, strip)
                val value = box(x, y, width, visual.y - gap - y)
                mapOf(
                    "SLOT_TITLE" to title, "SLOT_TOP" to top, "SLOT_ICON" to square(value, 0.6),
                    "SLOT_VALUE" to value, "SLOT_CAPTION" to caption, "SLOT_BADGE" to badge, "SLOT_VISUAL" to visual,
                )
            }
            "FACE_LAYOUT_SPLIT" -> if (width >= height) {
                val side = min(height, width * 0.45)
                val visual = box(x, y + (height - side) / 2, side, side)
                val tx = x + side + gap
                val tw = width - side - gap
                val middle = y + height / 2
                val title = box(tx, middle - md, tw, md)
                mapOf(
                    "SLOT_TITLE" to title, "SLOT_VALUE" to title, "SLOT_TOP" to box(tx, y, tw, xs),
                    "SLOT_ICON" to square(visual, 0.8), "SLOT_CAPTION" to box(tx, middle, tw, s),
                    "SLOT_BADGE" to badge, "SLOT_VISUAL" to visual,
                )
            } else {
                val side = min(width, height * 0.55)
                val visual = box(x + (width - side) / 2, y, side, side)
                val title = box(x, y + side + gap, width, md)
                mapOf(
                    "SLOT_TITLE" to title, "SLOT_VALUE" to title, "SLOT_TOP" to top,
                    "SLOT_ICON" to square(visual, 0.8), "SLOT_CAPTION" to box(x, title.y + title.h, width, s),
                    "SLOT_BADGE" to badge, "SLOT_VISUAL" to visual,
                )
            }
            "FACE_LAYOUT_DIAL" -> {
                val caption = box(x, y + height - s, width, s)
                val room = caption.y - gap - y
                val side = min(width, room)
                val visual = box(x + (width - side) / 2, y + (room - side) / 2, side, side)
                val value = square(visual, 0.6)
                mapOf(
                    "SLOT_TITLE" to caption, "SLOT_TOP" to top, "SLOT_ICON" to value, "SLOT_VALUE" to value,
                    "SLOT_CAPTION" to caption, "SLOT_BADGE" to badge, "SLOT_VISUAL" to visual,
                )
            }
            else -> {
                val title = box(x, y + height - md, width, md)
                val area = box(x, y, width, title.y - gap - y)
                mapOf(
                    "SLOT_TITLE" to title, "SLOT_CAPTION" to title, "SLOT_TOP" to top,
                    "SLOT_ICON" to square(area, 0.7), "SLOT_VALUE" to area, "SLOT_BADGE" to badge, "SLOT_VISUAL" to area,
                )
            }
        }
    }

    fun style(layout: String, slot: String): PlaceStyle {
        val leading = layout == "FACE_LAYOUT_ART" || layout == "FACE_LAYOUT_SPLIT"
        val align = if (leading) "TEXT_ALIGN_START" else "TEXT_ALIGN_CENTER"
        return when (slot) {
            "SLOT_VALUE" -> PlaceStyle("TEXT_SIZE_FIT", align, bold = true, maxLines = 1, quiet = false)
            "SLOT_CAPTION" -> PlaceStyle("TEXT_SIZE_S", align, bold = false, maxLines = 1, quiet = true)
            "SLOT_TOP" -> PlaceStyle("TEXT_SIZE_XS", align, bold = false, maxLines = 1, quiet = true)
            "SLOT_BADGE" -> PlaceStyle("TEXT_SIZE_XS", "TEXT_ALIGN_CENTER", bold = true, maxLines = 1, quiet = false)
            else -> if (layout == "FACE_LAYOUT_VALUE") {
                PlaceStyle("TEXT_SIZE_S", align, bold = false, maxLines = 1, quiet = true)
            } else {
                PlaceStyle("TEXT_SIZE_M", align, bold = leading, maxLines = if (layout == "FACE_LAYOUT_ICON_LABEL") 2 else 1, quiet = false)
            }
        }
    }
}

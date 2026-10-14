package dev.flexdeck.app

import android.content.Context
import android.content.res.Configuration
import android.graphics.Bitmap
import android.graphics.BitmapShader
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.LinearGradient
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import android.graphics.Shader
import android.graphics.Typeface
import android.os.SystemClock
import android.view.MotionEvent
import android.view.VelocityTracker
import android.view.View
import android.view.ViewConfiguration
import dev.flexdeck.client.BackgroundSpec
import dev.flexdeck.client.Box
import dev.flexdeck.client.Colors
import dev.flexdeck.client.Deck
import dev.flexdeck.client.FaceLayout
import dev.flexdeck.client.FaceSpec
import dev.flexdeck.client.ZoneSpec
import dev.flexdeck.client.Fired
import dev.flexdeck.client.Gesture
import dev.flexdeck.client.GestureRecognizer
import dev.flexdeck.client.Icons
import dev.flexdeck.client.LiveWidget
import dev.flexdeck.client.Pager
import dev.flexdeck.client.ScreenCare
import dev.flexdeck.client.PathData
import dev.flexdeck.client.Segment
import dev.flexdeck.client.SliderMath
import dev.flexdeck.client.SliderOrientation
import dev.flexdeck.client.SliderSpec
import dev.flexdeck.client.Spring
import dev.flexdeck.client.ValuePacer
import dev.flexdeck.client.WidgetKind
import dev.flexdeck.grid.GridLayout
import dev.flexdeck.grid.Orientation
import dev.flexdeck.grid.PlacedWidget
import dev.flexdeck.grid.Rect

/**
 * Draws a deck and reports touches.
 *
 * The rectangles come from [GridLayout] — the same algorithm, and the same
 * golden fixtures, as the web preview and the agent. Nothing here computes a
 * position of its own: a renderer that placed widgets by its own rules would
 * disagree with the editor by a few pixels and the preview would be a lie.
 *
 * Plain [View] rather than anything modern on purpose. The tablets this exists
 * for have about 1 GB of RAM and no GPU to spare: rounded rectangles, one
 * gradient per key, text — and frames only while something is actually moving.
 *
 * # Feel
 *
 * Keys are drawn as hardware: a face on a base, travelling down when pressed.
 * The travel is local and immediate — it does not wait for the agent — and
 * springs back on release. The palette is the editor's `--fd-deck-*` tokens.
 *
 * # Touch
 *
 * Every finger is tracked on its own, so two faders can be pulled at once and a
 * push-to-talk key can be held while another is tapped. [GestureRecognizer]
 * decides what a press was: a plain key fires the moment a finger lands; only a
 * key with a long press or double tap waits to see which.
 */
class DeckView(context: Context) : View(context) {

    /** Called with a widget id when a key is tapped. */
    var onPress: ((String) -> Unit)? = null

    /** Called with a widget id and a gesture name: double tap, long press, hold start and end. */
    var onGesture: ((String, String) -> Unit)? = null

    /** Called with a widget id and a zone's name when a button of its strip is tapped. */
    var onZone: ((String, String) -> Unit)? = null

    /** Which zone of each key's strip a finger is on, by widget id: drawn sunk. */
    private val zoneDown = HashMap<String, Int>()

    /**
     * Called as a slider moves: its id, the value, which drag this is, and
     * whether the finger has lifted. Already paced; send it as it comes.
     */
    var onSlide: ((String, Double, Int, Boolean) -> Unit)? = null

    /** Asked for an icon pack (`pack:` icon) this view does not have yet. */
    var onNeedPack: ((String) -> Unit)? = null

    /** Installed packs' icons, ready to draw: paths built once. */
    private class DrawnPackIcon(val image: String?, val paths: List<Path>, val view: List<Float>, val fill: Boolean, val stroke: Float)
    private val packs = HashMap<String, Map<String, DrawnPackIcon>>()
    private val requestedPacks = HashSet<String>()
    private val packPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        strokeCap = Paint.Cap.ROUND
        strokeJoin = Paint.Join.ROUND
    }

    /** A pack arrived (or could not: `null`, and it is asked for again later). */
    fun showPack(id: String, icons: Map<String, dev.flexdeck.client.Icons.PackIcon>?) {
        if (icons == null) {
            requestedPacks.remove(id)
            return
        }
        packs[id] = icons.mapValues { (_, icon) ->
            DrawnPackIcon(icon.image, icon.paths.map(::toPath), icon.view, icon.fill, icon.stroke ?: 2f)
        }
        invalidate()
    }

    /** Asked for a picture (`img:` icon) this view does not have yet. */
    var onNeedImage: ((String) -> Unit)? = null

    /** The bundled icons, by name, as drawable paths on a 24×24 grid. */
    private var iconPaths: Map<String, List<Path>> = emptyMap()

    /** Pictures by sha256, decoded at a button's size. Small: a deck shows few. */
    private val pictures = object : android.util.LruCache<String, Bitmap>(24) {}
    private val requested = HashSet<String>()

    /** The icon set, from the `icons.json` asset. */
    fun setIcons(icons: Map<String, List<String>>) {
        iconPaths = icons.mapValues { (_, paths) -> paths.map(::toPath) }
        invalidate()
    }

    /** A picture arrived (or could not: `null`). Call on the main thread. */
    fun showImage(sha256: String, bitmap: Bitmap?) {
        if (bitmap != null) pictures.put(sha256, bitmap) else requested.remove(sha256)
        invalidate()
    }

    private fun toPath(data: String): Path = Path().apply {
        for (segment in PathData.parse(data)) {
            when (segment) {
                is Segment.Move -> moveTo(segment.x, segment.y)
                is Segment.Line -> lineTo(segment.x, segment.y)
                is Segment.Cubic -> cubicTo(segment.x1, segment.y1, segment.x2, segment.y2, segment.x, segment.y)
                Segment.Close -> close()
            }
        }
    }

    private var deck: Deck? = null

    /**
     * The page on screen, by id as well as position, so a page added or moved
     * in the editor does not swap the one being looked at.
     */
    private var pageIndex = 0
    private var pageId: String? = null

    /** The page on screen, by id; `null` before a deck arrives. */
    val currentPageId: String? get() = pageId

    /** The person turned to a page here — not one the agent asked for. */
    var onPageShown: ((pageId: String) -> Unit)? = null

    /** How far the page is slid sideways, in screen pixels: follows a swipe, then springs home. */
    private val slide = Spring(0f, stiffness = 320f, damping = 36f)

    /**
     * A finger that may yet turn out to be a swipe between pages.
     *
     * Only one that lands off every key: a plain key fires the moment it is
     * touched, so a swipe starting on one would already have run it. Pages turn
     * from the page bar along the bottom and from the gaps between keys.
     */
    private class Swipe(val pointer: Int, val startX: Float, val startY: Float) {
        var claimed = false
        var dx = 0f
    }
    private var swipe: Swipe? = null

    /** Draws key faces. Icons and pictures come from this view's caches. */
    private val painter = FacePainter(
        icon = { canvas, value, left, top, size, color -> drawIconAt(canvas, value, left, top, size, color) },
        picture = { sha ->
            pictures.get(sha) ?: run {
                if (requested.add(sha)) onNeedImage?.invoke(sha)
                null
            }
        },
    )

    /** Each graphed value's recent history. */
    private var series: Map<String, List<Double>> = emptyMap()

    /** The page's background, behind every key, and the grid it covers. */
    private var wallpaper = BackgroundSpec()
    private val gridArea = RectF()

    /** When a clock or a pulse next needs a frame; 0 for none pending. */
    private var tickAt = 0L
    private val tickRunnable = Runnable {
        tickAt = 0L
        invalidate()
    }
    private var velocity: VelocityTracker? = null
    private val touchSlop = ViewConfiguration.get(context).scaledTouchSlop.toFloat()
    private val dotPaint = Paint(Paint.ANTI_ALIAS_FLAG)
    /** The page bar's name and position. */
    private val pagePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        typeface = Typeface.MONOSPACE
        letterSpacing = 0.12f
        textSize = 11f * resources.displayMetrics.density
    }

    /** Live state of bound widgets, from the agent. See `DeckSession.liveWidgets`. */
    private var live: Map<String, LiveWidget> = emptyMap()
    private var rectangles: Map<String, Rect> = emptyMap()

    /**
     * Everything to draw, worked out when the deck changes — so `onDraw`, which
     * runs every frame while a key is springing, allocates nothing.
     */
    private var drawn: List<DrawnWidget> = emptyList()

    // The device-accurate grid is scaled to fit whatever screen it lands on. The
    // *numbers* are never scaled — only the drawing of them.
    private var scale = 1f
    private var originX = 0f
    private var originY = 0f

    /** The deck's unit: 1 at a 1280px long edge, as the label size is. Radii and travel scale with it. */
    private var unit = 1f

    /** What each finger on the glass is doing, by pointer id. */
    private val touches = HashMap<Int, Touch>()

    /** Which gesture a press turns out to be. */
    private val recognizer = GestureRecognizer()
    private val tick = Runnable { fire(recognizer.tick(SystemClock.uptimeMillis())) }

    /** How far down each key is, 0 resting to 1 fully pressed. */
    private val travel = HashMap<String, Spring>()

    /** 0 while the agent is here, 1 while it is away: dims the deck and raises the notice. */
    private val away = Spring(0f, stiffness = 120f, damping = 22f)
    private var awayText = ""
    private var lastFrame = 0L

    /**
     * Where each slider's thumb was put by a finger, and when that finger
     * lifted. The agent's value is ignored while a finger is on it and for a
     * moment after, or the thumb would jump back to a value from mid-drag.
     */
    private val dragged = HashMap<String, Double>()
    private val releasedAt = HashMap<String, Long>()
    private var drags = 0

    private val background = Paint().apply {
        color = BACKGROUND
        shader = dotGrid()
    }
    private val basePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = KEY_BASE }
    private val facePaint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val edgePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeWidth = 1.5f
    }
    private val glintPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = GLINT
        style = Paint.Style.STROKE
        strokeWidth = 1.5f
    }
    private val ledPaint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val ledGlow = Paint(Paint.ANTI_ALIAS_FLAG)
    private val labelPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = INK
        textAlign = Paint.Align.CENTER
        typeface = Typeface.create(Typeface.SANS_SERIF, Typeface.NORMAL)
    }
    private val monoPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        textAlign = Paint.Align.CENTER
        typeface = Typeface.MONOSPACE
        letterSpacing = 0.2f
    }
    private val trackPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = TRACK }
    private val trackEdge = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = KEY_LINE
        style = Paint.Style.STROKE
        strokeWidth = 1.5f
    }
    private val levelPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = LEVEL }
    private val thumbPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = THUMB }
    private val thumbShadowPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = THUMB_SHADOW }
    private val dimPaint = Paint()
    private val platePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = PLATE }
    private val scratch = RectF()
    private val scratch2 = RectF()
    private val iconPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeWidth = 1.6f
        strokeCap = Paint.Cap.ROUND
        strokeJoin = Paint.Join.ROUND
    }
    private val picturePaint = Paint(Paint.FILTER_BITMAP_FLAG)
    private val ringPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeWidth = 3f
    }
    private val unavailablePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = UNAVAILABLE_SHADE }
    private val warnPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeCap = Paint.Cap.ROUND
        strokeJoin = Paint.Join.ROUND
        color = WAIT
    }
    private val warnPath = Path()

    /**
     * A widget reduced to exactly what drawing it needs.
     *
     * The frame, the colours, the face's gradient and every look are
     * precomputed, so `onDraw` allocates and parses nothing at all.
     */
    private class DrawnWidget(
        val id: String,
        val frame: RectF,
        val label: String,
        /** A solid fill the author chose, or `null` for the machined face. */
        val fill: Int?,
        val text: Int,
        val face: Shader,
        val looks: Map<String, DrawnLook>,
        val icon: String?,
        val slider: SliderSpec?,
        val vertical: Boolean,
        val gestures: Set<String>,
        /** A slider's level colour: its foreground colour if it has one. */
        val accent: Int = LEVEL,
        /** A slider's icon-and-name row: a tap there is its mute. */
        val header: RectF = RectF(),
        /** A slider's track, end to end; the thumb's centre runs along it. */
        val track: RectF = RectF(),
        val thumbRadius: Float = 0f,
        /** The face of each look, its own under "". */
        val faces: Map<String, FaceSpec> = emptyMap(),
        /** Where each face's places fall on this key, by look id. */
        val places: Map<String, Map<String, Box>> = emptyMap(),
        /** Its own face is the one schema 3 drew from its props. */
        val derived: Boolean = true,
        /** Names a variable as the agent does: `this.` is its own source run. */
        val resolve: (String) -> String = { it },
        /** Its strip's buttons, and where each falls, in grid units. */
        val zones: List<ZoneSpec> = emptyList(),
        val zoneRects: List<RectF> = emptyList(),
        /** No light in the corner while it is lit. */
        val hideDot: Boolean = false,
    )

    /** A look, resolved against its widget's own colours and label. */
    private class DrawnLook(val label: String, val fill: Int?, val text: Int, val icon: String?)

    /**
     * One finger: which widget it landed on, and for a slider, its pacing and
     * where the drag started — a fader moves by how far the finger goes, not to
     * where it landed.
     */
    private class Touch(
        val widget: DrawnWidget,
        val drag: Int,
        val pacer: ValuePacer?,
        val startValue: Double = 0.0,
        val startPos: Float = 0f,
        /** The zone of the key's strip it landed on; -1: the key itself. */
        val zone: Int = -1,
    ) {
        /** Whether the value has left where it started: a touch that never moved sends nothing. */
        var moved = false
    }

    /**
     * Show bound widgets' live state. Cheap: it only swaps the map and redraws,
     * because these arrive once a second while a timecode is on screen.
     */
    fun showLive(states: Map<String, LiveWidget>) {
        live = states.toMap()
        invalidate()
    }

    /**
     * Whether the agent is here. Away, the deck stays — a tablet that goes
     * blank when the PC sleeps looks broken — but dims under a notice, and
     * presses are not taken, so nothing is sent into the void.
     */
    fun setConnected(connected: Boolean, notice: String = "") {
        awayText = notice
        val target = if (connected) 0f else 1f
        if (away.target != target) {
            away.target = target
            if (!connected) releaseAll()
            wake()
        } else {
            invalidate()
        }
    }

    fun show(deck: Deck?) {
        this.deck = deck

        // Stay on the page being looked at: by id first, else by position.
        if (deck != null) {
            pageIndex = deck.indexOf(pageId) ?: pageIndex.coerceIn(0, deck.pages.size - 1)
            pageId = deck.pages[pageIndex].id
        }
        val shown = deck?.pages?.get(pageIndex)?.widgets.orEmpty()

        // Landscape and portrait are two layouts: each has its own grid, and each
        // widget its own place — pinned in the editor, or assumed until it is.
        // `placePage` is the same rule the editor's preview uses (packages/grid),
        // held together by the same tests, so the two cannot disagree.
        val orientation = orientation()
        val page = deck?.let {
            GridLayout.placePage(
                it.grid,
                it.portraitGrid,
                shown.map { widget -> PlacedWidget(widget.span, widget.portrait) },
                orientation,
            )
        }

        this.rectangles = page?.let { placed -> GridLayout.layoutPage(placed.spec, placed.widgets.map { it.span }) } ?: emptyMap()

        if (deck == null || page == null || page.spec.usableW <= 0 || page.spec.usableH <= 0) {
            scale = 1f
            originX = 0f
            originY = 0f
            drawn = emptyList()
        } else {
            // Fit, never stretch: a deck drawn at the wrong aspect ratio would
            // hit the wrong widget when touched.
            // Several pages: the deck gives up a strip along the bottom to the page bar.
            val room = height - pageBarHeight()
            scale = minOf(
                width.toFloat() / page.spec.usableW,
                room / page.spec.usableH,
            )
            originX = (width - page.spec.usableW * scale) / 2f
            originY = (room - page.spec.usableH * scale) / 2f

            // By the panel's LONG edge, not `usableW`: that is the short edge in
            // portrait, and labels would shrink every time the tablet turned.
            unit = (maxOf(page.spec.usableW, page.spec.usableH) / 1280f).coerceIn(0.7f, 1.4f)
            labelPaint.textSize = 16f * unit
            painter.unit = unit
            painter.reset()
            wallpaper = deck.pages[pageIndex].background
            gridArea.set(0f, 0f, page.spec.usableW.toFloat(), page.spec.usableH.toFloat())

            // The deck's OWN widgets rather than the oriented spans: ids survive the
            // transpose, so the rectangles above are already the right ones.
            drawn = shown.mapNotNull { widget ->
                val rect = rectangles[widget.id] ?: return@mapNotNull null

                val fill = Colors.parse(widget.background)
                val text = Colors.parse(widget.foreground) ?: INK
                val frame = RectF(
                    rect.x.toFloat(),
                    rect.y.toFloat(),
                    (rect.x + rect.w).toFloat(),
                    (rect.y + rect.h).toFloat(),
                )
                val slider = widget.slider.takeIf { widget.kind == WidgetKind.SLIDER }
                // A deck turned to portrait is transposed, so a slider authored
                // across it now runs up it, and vice versa.
                val vertical = slider?.let {
                    SliderMath.isVertical(oriented(it, orientation), frame.width(), frame.height())
                } ?: false
                val header = RectF()
                val track = RectF()
                val thumb = if (slider != null) sliderGeometry(frame, vertical, header, track) else 0f
                val strip = FaceLayout.placeZones(
                    if (slider == null) widget.zones.size else 0,
                    frame.width().toDouble(),
                    frame.height().toDouble(),
                    unit.toDouble(),
                )
                DrawnWidget(
                    widget.id,
                    frame,
                    widget.label,
                    fill,
                    text,
                    LinearGradient(0f, frame.top, 0f, frame.bottom, KEY_TOP, KEY_BOTTOM, Shader.TileMode.CLAMP),
                    widget.looks.mapValues { (_, look) ->
                        DrawnLook(
                            label = look.label ?: widget.label,
                            fill = Colors.parse(look.background) ?: fill,
                            text = Colors.parse(look.foreground) ?: text,
                            icon = look.icon ?: widget.icon,
                        )
                    },
                    icon = widget.icon,
                    slider = slider,
                    vertical = vertical,
                    gestures = widget.gestures,
                    accent = Colors.parse(widget.foreground) ?: LEVEL,
                    header = header,
                    track = track,
                    thumbRadius = thumb,
                    faces = widget.faces,
                    places = widget.faces.mapValues { (_, face) ->
                        // With a strip, the face is placed in the room above it.
                        val w = frame.width().toDouble()
                        val placed = FaceLayout.place(face.layout, w, strip.face.h, unit.toDouble())
                        // Elements put anywhere are found under "#<index>".
                        val free = face.elements.withIndex().mapNotNull { (index, element) ->
                            element.box?.let { "#$index" to FaceLayout.placeBox(it, w, strip.face.h) }
                        }
                        placed + free
                    },
                    derived = widget.derived,
                    resolve = widget::resolve,
                    zones = widget.zones,
                    zoneRects = strip.zones.map { z ->
                        RectF(frame.left + z.x.toFloat(), frame.top + z.y.toFloat(), frame.left + z.right.toFloat(), frame.top + z.bottom.toFloat())
                    },
                    hideDot = widget.hideDot,
                )
            }
        }

        invalidate()
    }

    /**
     * Lay out a slider tile in `frame`: the icon-and-name row at the top, a
     * recessed track below it, and room under a vertical track for the number
     * (a horizontal one puts the number at the end of the name row). Fills
     * `header` and `track`, and returns the thumb's half-size.
     */
    private fun sliderGeometry(frame: RectF, vertical: Boolean, header: RectF, track: RectF): Float {
        val text = labelPaint.textSize
        val inset = SLIDER_INSET * unit
        header.set(frame.left, frame.top, frame.right, frame.top + inset + text * 1.9f)
        return if (vertical) {
            val thumb = (frame.width() * 0.16f).coerceIn(10f, 26f)
            val half = (frame.width() * 0.3f).coerceAtMost(48f * unit)
            val bottom = frame.bottom - inset - text * 1.5f - thumb * 0.5f
            track.set(frame.centerX() - half, header.bottom + thumb * 0.5f, frame.centerX() + half, maxOf(bottom, header.bottom + thumb))
            thumb
        } else {
            val thumb = ((frame.bottom - header.bottom) * 0.3f).coerceIn(8f, 26f)
            val half = ((frame.bottom - header.bottom) * 0.3f).coerceAtMost(40f * unit)
            val centre = (header.bottom + frame.bottom - inset * 0.5f) / 2f
            track.set(frame.left + inset + thumb * 0.5f, centre - half, frame.right - inset - thumb * 0.5f, centre + half)
            thumb
        }
    }

    private fun oriented(spec: SliderSpec, orientation: Orientation): SliderSpec =
        if (orientation != Orientation.PORTRAIT) {
            spec
        } else {
            when (spec.orientation) {
                SliderOrientation.HORIZONTAL -> spec.copy(orientation = SliderOrientation.VERTICAL)
                SliderOrientation.VERTICAL -> spec.copy(orientation = SliderOrientation.HORIZONTAL)
                SliderOrientation.AUTO -> spec
            }
        }

    /**
     * Which way up the panel currently is. Read from the resources because the
     * Activity handles its own configuration changes: on rotation this View is
     * merely resized, and the configuration is the only thing that knows.
     */
    private fun orientation(): Orientation =
        if (resources.configuration.orientation == Configuration.ORIENTATION_PORTRAIT) {
            Orientation.PORTRAIT
        } else {
            Orientation.LANDSCAPE
        }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        super.onSizeChanged(w, h, oldw, oldh)
        show(deck)
    }

    /** The value a slider's thumb sits at: the finger's while it is near, else the agent's. */
    private fun sliderValue(widget: DrawnWidget, spec: SliderSpec): Double {
        val local = dragged[widget.id]
        val held = touches.values.any { it.widget.id == widget.id }
        val settling = releasedAt[widget.id]?.let { SystemClock.uptimeMillis() - it < GRACE_MS } ?: false
        if (local != null && (held || settling)) return local
        return live[widget.id]?.value ?: local ?: spec.min
    }

    /** Ask for frames until every spring has come to rest. */
    private fun wake() {
        lastFrame = 0L
        postInvalidateOnAnimation()
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)

        // Step the springs by the real time since the last frame.
        val now = SystemClock.uptimeMillis()
        val dt = if (lastFrame == 0L) 1f / 60f else (now - lastFrame) / 1000f
        lastFrame = now
        var moving = away.step(dt)
        for (spring in travel.values) if (spring.step(dt)) moving = true
        // While a finger drags the page the spring is held where it is put.
        if (swipe?.claimed != true && slide.step(dt)) moving = true

        canvas.drawPaint(background)
        drawPageDots(canvas)

        // Against burn-in: the whole deck sits a pixel or two off centre, moving on every couple of minutes.
        val care = deck?.screen ?: ScreenCare()
        val (shiftX, shiftY) = if (care.shift) ScreenCare.offset(System.currentTimeMillis(), 2f * resources.displayMetrics.density) else 0f to 0f
        canvas.save()
        canvas.translate(originX + slide.value + shiftX, originY + shiftY)
        canvas.scale(scale, scale)

        // The page's own background, under every key.
        drawWallpaper(canvas)

        val radius = KEY_RADIUS * unit
        val drop = KEY_DROP * unit
        val wall = System.currentTimeMillis()
        var due: Long? = null
        for (widget in drawn) {
            val state = live[widget.id]
            // The look the agent names, if the widget has it; otherwise its own.
            val look = state?.stateId?.let { widget.looks[it] }
            val lookId = state?.stateId?.takeIf { widget.faces.containsKey(it) } ?: ""
            val face = widget.faces[lookId] ?: widget.faces[""]
            // "Light up while" draws the deck's glow rather than a colour.
            val lit = state?.stateId == LIT_STATE && widget.derived && state.background.isEmpty()
            val pressed = travel[widget.id]?.value ?: 0f
            val ink = look?.text ?: widget.text
            labelPaint.color = ink

            // The base the key travels into, then the face, pushed down by the press.
            val frame = widget.frame
            scratch.set(frame.left, frame.top + drop, frame.right, frame.bottom + drop)
            val spec = widget.slider
            val transparent = spec == null && face?.background?.transparent == true && state?.background.isNullOrEmpty()
            if (!transparent) canvas.drawRoundRect(scratch, radius, radius, basePaint)

            canvas.save()
            val shrink = 1f - 0.03f * pressed
            canvas.translate(0f, drop * pressed)
            canvas.scale(shrink, shrink, frame.centerX(), frame.centerY())

            if (spec != null || face == null) {
                val fill = look?.fill ?: widget.fill
                facePaint.shader = if (fill == null) widget.face else null
                if (fill != null) facePaint.color = fill
                canvas.drawRoundRect(frame, radius, radius, facePaint)
            } else if (lit) {
                facePaint.shader = null
                facePaint.color = LIT_FACE
                canvas.drawRoundRect(frame, radius, radius, facePaint)
            } else {
                val image = state?.backgroundImage?.takeIf { it.isNotEmpty() }
                    ?: face.background.image.takeIf { !it.contains("{{") }
                    ?: ""
                painter.background(
                    canvas, frame, radius, face.layout, face.background,
                    state?.background.orEmpty(), image,
                    face.background.graph.takeIf { it.isNotEmpty() }?.let { series[widget.resolve(it)] },
                    widget.face,
                )
            }
            edgePaint.color = if (lit) LIT_LINE else KEY_LINE
            if (!transparent) canvas.drawRoundRect(frame, radius, radius, edgePaint)
            if (pressed < 0.5f && !transparent) {
                // The glint along the top edge that makes a face read as raised.
                canvas.drawLine(frame.left + radius, frame.top + 1.5f, frame.right - radius, frame.top + 1.5f, glintPaint)
            }
            // A rule asking the whole key to pulse: a breathing ring.
            if (state?.pulse == true) {
                ringPaint.color = SIGNAL
                ringPaint.alpha = (painter.pulse(wall) * 255).toInt()
                canvas.drawRoundRect(frame, radius, radius, ringPaint)
                due = minOf(due ?: FacePainter.PULSE_FRAME_MS, FacePainter.PULSE_FRAME_MS)
            }

            val label = state?.label?.takeIf { it.isNotEmpty() } ?: look?.label ?: widget.label
            // A live icon (album art) outranks the look's, which outranks the widget's.
            val icon = state?.icon?.takeIf { it.isNotEmpty() } ?: look?.icon ?: widget.icon
            if (spec != null) {
                drawSlider(canvas, widget, spec, label, icon, muted = state?.stateId == MUTED_STATE)
            } else if (face != null) {
                val places = widget.places[lookId] ?: widget.places[""].orEmpty()
                painter.elements(
                    canvas, "${widget.id}/$lookId", frame, face, places,
                    state?.elements.orEmpty(),
                    legacyLabel = state?.label.orEmpty(),
                    legacyIcon = state?.icon?.takeIf { it.isNotEmpty() } ?: widget.icon.orEmpty(),
                    ink = if (lit) LIT_INK else ink,
                    series = series,
                    resolve = widget.resolve,
                    nowMs = wall,
                    fallbackTitle = widget.label,
                )?.let { due = minOf(due ?: it, it) }
            }

            if (spec == null && widget.zones.isNotEmpty()) drawZones(canvas, widget, state?.zonesOn.orEmpty(), if (lit) LIT_INK else ink, radius)

            // A source it reads is failing: an amber dot in the far corner, so
            // a stale number is never mistaken for a fresh one.
            if (spec == null && !state?.problem.isNullOrEmpty()) {
                ledPaint.color = WAIT
                canvas.drawCircle(frame.left + 12f * unit, frame.top + 12f * unit, 3.5f * unit, ledPaint)
            }

            // A plugin no longer has what it does: shaded, with a warning, and a
            // press is not sent. It comes back when the plugin does.
            if (!state?.unavailable.isNullOrEmpty()) {
                canvas.drawRoundRect(frame, radius, radius, unavailablePaint)
                drawWarning(canvas, frame.centerX(), frame.centerY(), minOf(frame.width(), frame.height()) * 0.18f)
            }

            // A light in the corner while the key is active — its scene is the live
            // one, its toggle is on. Every key does something, so that says nothing.
            if (spec == null && lit && !widget.hideDot) {
                val led = 3.5f * unit
                val x = frame.right - 12f * unit
                val y = frame.top + 12f * unit
                val color = OK
                ledGlow.color = color
                ledGlow.alpha = 70
                canvas.drawCircle(x, y, led * 2.2f, ledGlow)
                ledPaint.color = color
                canvas.drawCircle(x, y, led, ledPaint)
            }
            canvas.restore()
        }

        canvas.restore()

        if (away.value > 0.01f) drawAway(canvas, away.value)

        // Something pulsing wants every frame; a clock only its next tick; a
        // shifting deck its next move.
        val shiftDue = if (care.shift) ScreenCare.SHIFT_MS - System.currentTimeMillis() % ScreenCare.SHIFT_MS else null
        val wait = listOfNotNull(due, shiftDue).minOrNull()
        if (wait != null && wait <= FacePainter.PULSE_FRAME_MS) moving = true
        if (moving) {
            postInvalidateOnAnimation()
        } else {
            lastFrame = 0L
            if (wait != null) {
                val at = SystemClock.uptimeMillis() + wait.coerceAtLeast(1L)
                if (tickAt == 0L || at < tickAt) {
                    removeCallbacks(tickRunnable)
                    tickAt = at
                    postDelayed(tickRunnable, wait.coerceAtLeast(1L))
                }
            }
        }
    }

    /** The page's background: a colour, a fade or a picture across the whole deck. */
    private fun drawWallpaper(canvas: Canvas) {
        val spec = wallpaper
        if (spec.color.isEmpty() && spec.image.isEmpty()) return
        painter.background(
            canvas, gridArea, KEY_RADIUS * unit, "FACE_LAYOUT_ICON_LABEL", spec,
            "", spec.image.takeIf { !it.contains("{{") }.orEmpty(), null, background.shader,
        )
    }

    /** Each graphed value's recent history, as the session holds it. */
    fun showSeries(series: Map<String, List<Double>>) {
        this.series = series.mapValues { (_, points) -> points.toList() }
        invalidate()
    }

    /**
     * Show a page the agent asked for: by id, or so many pages on (wrapping).
     * Not reported back — the agent already knows, and an echo would set two
     * tablets bouncing each other between pages.
     */
    fun showPage(pageId: String, step: Int) {
        val deck = deck ?: return
        val count = deck.pages.size
        val target = if (pageId.isNotEmpty()) {
            deck.indexOf(pageId) ?: return
        } else {
            Math.floorMod(pageIndex + step, count)
        }
        turnTo(target, 0f, report = false)
    }

    /** The height of the page bar: none for a deck with one page. */
    private fun pageBarHeight(): Float =
        if ((deck?.pages?.size ?: 0) > 1) PAGE_BAR_DP * resources.displayMetrics.density else 0f

    /** Where page `index`'s dot sits along the bar, and the spacing between dots. */
    private fun dotX(index: Int, count: Int): Float {
        val pitch = PAGE_DOT_PITCH_DP * resources.displayMetrics.density
        return width / 2f + (index - (count - 1) / 2f) * pitch
    }

    /**
     * The page bar: a dot per page, the current one a lit pill — the same mark
     * the editor's page dots use. Drawn under the deck, so a sliding page passes
     * over nothing.
     */
    private fun drawPageDots(canvas: Canvas) {
        val count = deck?.pages?.size ?: 0
        if (count <= 1) return
        val density = resources.displayMetrics.density
        val y = height - pageBarHeight() / 2f
        val r = 3f * density
        for (index in 0 until count) {
            val x = dotX(index, count)
            if (index == pageIndex) {
                dotPaint.color = SIGNAL
                scratch2.set(x - r * 3f, y - r, x + r * 3f, y + r)
                canvas.drawRoundRect(scratch2, r, r, dotPaint)
            } else {
                dotPaint.color = MUTED_LEVEL
                canvas.drawCircle(x, y, r, dotPaint)
            }
        }

        // Which page, in words and in place: the name to the left of the dots,
        // "2 / 5" to the right. A name that would reach the dots is cut short.
        val gutter = 16f * density
        val baseline = y + pagePaint.textSize * 0.35f
        val dotsLeft = dotX(0, count) - r * 3f - gutter
        val name = deck?.pages?.get(pageIndex)?.name.orEmpty().ifBlank { "Page ${pageIndex + 1}" }.uppercase()
        pagePaint.textAlign = Paint.Align.LEFT
        pagePaint.color = INK
        val room = dotsLeft - gutter
        if (room > 0f) {
            val fits = pagePaint.breakText(name, true, room, null)
            val shown = if (fits < name.length) name.take((fits - 1).coerceAtLeast(0)) + "…" else name
            canvas.drawText(shown, gutter, baseline, pagePaint)
        }
        pagePaint.textAlign = Paint.Align.RIGHT
        pagePaint.color = MUTED_INK
        canvas.drawText("${pageIndex + 1} / $count", width - gutter, baseline, pagePaint)
    }

    /** The page whose dot is nearest screen `x`. */
    private fun pageAtDot(x: Float, count: Int): Int {
        val pitch = PAGE_DOT_PITCH_DP * resources.displayMetrics.density
        val nearest = Math.round((x - dotX(0, count)) / pitch)
        return nearest.coerceIn(0, count - 1)
    }

    /**
     * Go to page `target`, the new page sliding in from the side it lies on.
     * `from` is where the old page was showing, so a swipe hands over smoothly.
     */
    private fun turnTo(target: Int, from: Float, report: Boolean = true) {
        val deck = deck ?: return
        if (target == pageIndex) {
            slide.snap(from)
        } else {
            val direction = if (target > pageIndex) 1 else -1
            // Fingers on the old page's keys let go of them first.
            releaseAll()
            pageIndex = target
            pageId = deck.pages[target].id
            show(deck)
            slide.snap(from + direction * width)
            if (report) onPageShown?.invoke(deck.pages[target].id)
        }
        slide.target = 0f
        wake()
    }

    /**
     * The deck while the agent is away: dimmed, and a plate in the middle saying
     * so — and when the next attempt is — so a sleeping PC never reads as a
     * broken tablet.
     */
    private fun drawAway(canvas: Canvas, amount: Float) {
        dimPaint.color = Color.argb((amount * 170).toInt(), 5, 5, 6)
        canvas.drawPaint(dimPaint)

        val plateW = minOf(width * 0.7f, 560f * resources.displayMetrics.density / 1.5f)
        val plateH = plateW * 0.34f
        val cx = width / 2f
        val cy = height / 2f + (1f - amount) * 24f
        scratch2.set(cx - plateW / 2f, cy - plateH / 2f, cx + plateW / 2f, cy + plateH / 2f)
        platePaint.alpha = (amount * 255).toInt()
        canvas.drawRoundRect(scratch2, 28f, 28f, platePaint)
        edgePaint.color = KEY_LINE
        canvas.drawRoundRect(scratch2, 28f, 28f, edgePaint)

        monoPaint.color = SIGNAL
        monoPaint.alpha = (amount * 255).toInt()
        monoPaint.textSize = plateH * 0.26f
        monoPaint.isFakeBoldText = true
        canvas.drawText("SIGNAL LOST", cx, cy - plateH * 0.02f, monoPaint)
        monoPaint.isFakeBoldText = false

        // A light that breathes while it waits.
        val breathe = 0.35f + 0.65f * ((Math.sin(SystemClock.uptimeMillis() / 300.0) + 1.0) / 2.0).toFloat()
        monoPaint.textSize = plateH * 0.1f
        monoPaint.color = MUTED_INK
        monoPaint.alpha = (amount * 255).toInt()
        val text = awayText.uppercase()
        val textWidth = monoPaint.measureText(text)
        val ledX = cx - textWidth / 2f - monoPaint.textSize
        val ledY = cy + plateH * 0.25f - monoPaint.textSize * 0.35f
        ledPaint.color = WAIT
        ledPaint.alpha = (breathe * amount * 255).toInt()
        canvas.drawCircle(ledX, ledY, monoPaint.textSize * 0.35f, ledPaint)
        ledPaint.alpha = 255
        canvas.drawText(text, cx + monoPaint.textSize * 0.4f, cy + plateH * 0.25f, monoPaint)

        // Keep breathing while the notice is up.
        postInvalidateOnAnimation()
    }

    /**
     * A slider tile, the way a mixer channel draws one: its icon and name along
     * the top (tap there to mute), a recessed track filled to the value in the
     * signal colour, a machined thumb, and the number. Muted, the level greys.
     */
    private fun drawSlider(
        canvas: Canvas,
        widget: DrawnWidget,
        spec: SliderSpec,
        label: String,
        icon: String?,
        muted: Boolean,
    ) {
        val value = sliderValue(widget, spec)
        val fraction = SliderMath.fraction(spec, value)
        val track = widget.track
        val header = widget.header
        val text = labelPaint.textSize
        val inset = SLIDER_INSET * unit
        val number = formatValue(value)
        val ink = labelPaint.color

        // The name row: icon then name, centred on a vertical tile and to the
        // left of the number on a horizontal one.
        val iconSize = text * 1.25f
        val gap = text * 0.35f
        val rowY = header.top + inset + iconSize / 2f
        val numberWidth = if (widget.vertical) 0f else labelPaint.measureText(number) + gap * 2f
        val room = header.width() - inset * 2f - iconSize - gap - numberWidth
        val name = fit(label, room)
        val nameWidth = labelPaint.measureText(name)
        val hasIcon = Icons.source(icon) != null
        val rowWidth = (if (hasIcon) iconSize + gap else 0f) + nameWidth
        val rowLeft = if (widget.vertical) header.centerX() - rowWidth / 2f else header.left + inset
        val drewIcon = hasIcon && drawIconAt(canvas, icon, rowLeft, rowY - iconSize / 2f, iconSize, if (muted) MUTED_INK else ink)
        labelPaint.textAlign = Paint.Align.LEFT
        if (muted) labelPaint.color = MUTED_INK
        canvas.drawText(name, if (drewIcon) rowLeft + iconSize + gap else rowLeft, rowY + text / 3f, labelPaint)

        // The recessed track, the level up to the thumb, and the thumb.
        val radius = minOf(minOf(track.width(), track.height()) / 2f, 14f * unit)
        canvas.drawRoundRect(track, radius, radius, trackPaint)
        canvas.drawRoundRect(track, radius, radius, trackEdge)
        levelPaint.color = if (muted) MUTED_LEVEL else widget.accent
        val thumbX: Float
        val thumbY: Float
        if (widget.vertical) {
            thumbX = track.centerX()
            thumbY = track.bottom - track.height() * fraction
            scratch.set(track.left, thumbY, track.right, track.bottom)
        } else {
            thumbX = track.left + track.width() * fraction
            thumbY = track.centerY()
            scratch.set(track.left, track.top, thumbX, track.bottom)
        }
        if (fraction > 0f) {
            levelPaint.alpha = 200
            canvas.drawRoundRect(scratch, radius, radius, levelPaint)
            levelPaint.alpha = 255
        }
        // A machined thumb: a light bar across the track, a groove in its middle.
        val t = widget.thumbRadius
        if (widget.vertical) {
            scratch.set(track.left - 4f * unit, thumbY - t * 0.45f, track.right + 4f * unit, thumbY + t * 0.45f)
        } else {
            scratch.set(thumbX - t * 0.45f, track.top - 4f * unit, thumbX + t * 0.45f, track.bottom + 4f * unit)
        }
        scratch2.set(scratch.left, scratch.top + 3f * unit, scratch.right, scratch.bottom + 3f * unit)
        canvas.drawRoundRect(scratch2, 7f * unit, 7f * unit, thumbShadowPaint)
        canvas.drawRoundRect(scratch, 7f * unit, 7f * unit, thumbPaint)

        // The number, in the signal colour: under a vertical track, at the end of a horizontal one's row.
        labelPaint.color = if (muted) MUTED_INK else NUMBER
        labelPaint.isFakeBoldText = true
        if (widget.vertical) {
            labelPaint.textAlign = Paint.Align.CENTER
            canvas.drawText(if (muted) "MUTED" else number, widget.frame.centerX(), widget.frame.bottom - inset - text * 0.3f, labelPaint)
        } else {
            labelPaint.textAlign = Paint.Align.RIGHT
            canvas.drawText(if (muted) "MUTED" else number, header.right - inset, rowY + text / 3f, labelPaint)
        }
        labelPaint.isFakeBoldText = false
        labelPaint.textAlign = Paint.Align.CENTER
        labelPaint.color = ink
    }

    /** `text`, cut short with an ellipsis to fit `width` in the label paint. */
    private fun fit(text: String, width: Float): String {
        if (width <= 0f) return ""
        if (labelPaint.measureText(text) <= width) return text
        val count = labelPaint.breakText(text, true, width - labelPaint.measureText("…"), null)
        return if (count <= 0) "" else text.substring(0, count).trimEnd() + "…"
    }

    /**
     * Draw a widget's icon in the space above its label. Returns whether there
     * was one to draw — a picture still on its way counts as none, and is asked
     * for once.
     */
    private fun drawIcon(canvas: Canvas, frame: RectF, value: String?, color: Int): Boolean {
        val size = minOf(frame.width() * 0.4f, frame.height() - labelPaint.textSize * 2.6f)
        if (size < 12f) return false
        val left = frame.centerX() - size / 2f
        val top = frame.top + (frame.height() - labelPaint.textSize * 1.8f - size) / 2f
        return drawIconAt(canvas, value, left, top, size, color)
    }

    /**
     * A key's strip of zones: a shade across its bottom, a hairline between
     * buttons, each button's icon — its other one while its `when` holds — and
     * the one under a finger sunk.
     */
    private fun drawZones(canvas: Canvas, widget: DrawnWidget, on: Set<String>, ink: Int, radius: Float) {
        val first = widget.zoneRects.firstOrNull() ?: return
        val strip = RectF(widget.frame.left, first.top, widget.frame.right, widget.frame.bottom)
        canvas.save()
        canvas.clipRect(strip)
        zoneShade.color = ZONE_SHADE
        canvas.drawRoundRect(widget.frame, radius, radius, zoneShade)
        val down = zoneDown[widget.id]
        widget.zoneRects.forEachIndexed { index, rect ->
            if (index == down) {
                zoneShade.color = ZONE_DOWN
                canvas.drawRect(rect, zoneShade)
            }
            if (index > 0) canvas.drawLine(rect.left, rect.top + rect.height() * 0.2f, rect.left, rect.bottom - rect.height() * 0.2f, zoneLine)
            val zone = widget.zones[index]
            val icon = zone.iconOn.takeIf { zone.name in on && it.isNotEmpty() } ?: zone.icon
            val size = minOf(rect.width(), rect.height()) * 0.5f
            drawIconAt(canvas, icon, rect.centerX() - size / 2f, rect.centerY() - size / 2f, size, ink)
        }
        canvas.drawLine(strip.left, strip.top, strip.right, strip.top, zoneLine)
        canvas.restore()
    }

    private val zoneShade = Paint(Paint.ANTI_ALIAS_FLAG)
    private val zoneLine = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = ZONE_LINE
        strokeWidth = 1f
    }

    /** Draw an icon `size` square at (`left`, `top`). Returns whether there was one. */
    private fun drawIconAt(canvas: Canvas, value: String?, left: Float, top: Float, size: Float, color: Int): Boolean {
        val source = Icons.source(value) ?: return false
        when (source) {
            is Icons.Source.Bundled -> {
                val paths = iconPaths[source.name] ?: return false
                iconPaint.color = color
                canvas.save()
                canvas.translate(left, top)
                canvas.scale(size / 24f, size / 24f)
                for (path in paths) canvas.drawPath(path, iconPaint)
                canvas.restore()
            }
            is Icons.Source.Image -> {
                val picture = pictures.get(source.sha256)
                if (picture == null) {
                    if (requested.add(source.sha256)) onNeedImage?.invoke(source.sha256)
                    return false
                }
                scratch.set(left, top, left + size, top + size)
                canvas.drawBitmap(picture, null, scratch, picturePaint)
            }
            is Icons.Source.Pack -> {
                val pack = packs[source.pack]
                if (pack == null) {
                    if (requestedPacks.add(source.pack)) onNeedPack?.invoke(source.pack)
                    return false
                }
                val icon = pack[source.name] ?: return false
                icon.image?.let { return drawIconAt(canvas, it, left, top, size, color) }
                // Paths on the icon's own view box, scaled to fit `size`, centred.
                val (vx, vy, vw, vh) = icon.view
                val scale = size / maxOf(vw, vh)
                canvas.save()
                canvas.translate(left + (size - vw * scale) / 2f, top + (size - vh * scale) / 2f)
                canvas.scale(scale, scale)
                canvas.translate(-vx, -vy)
                packPaint.color = color
                packPaint.style = if (icon.fill) Paint.Style.FILL else Paint.Style.STROKE
                packPaint.strokeWidth = icon.stroke
                for (path in icon.paths) canvas.drawPath(path, packPaint)
                canvas.restore()
            }
        }
        return true
    }

    private fun formatValue(value: Double): String =
        if (value == Math.floor(value) && Math.abs(value) < 1e9) value.toLong().toString()
        else String.format(java.util.Locale.ROOT, "%.1f", value)

    override fun onTouchEvent(event: MotionEvent): Boolean {
        val now = SystemClock.uptimeMillis()
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> {
                // Nothing is sent while the agent is away; the notice says why.
                if (away.target > 0f) return true
                // Mid-swipe the page is moving: no key under it can be meant.
                if (swipe?.claimed == true) return true
                val index = event.actionIndex
                val pointer = event.getPointerId(index)
                val widget = widgetAt(event.getX(index), event.getY(index))
                if (event.actionMasked == MotionEvent.ACTION_DOWN) {
                    velocity?.recycle()
                    velocity = VelocityTracker.obtain().also { it.addMovement(event) }
                    val paged = (deck?.pages?.size ?: 0) > 1
                    swipe = if (paged && widget == null) Swipe(pointer, event.getX(index), event.getY(index)) else null
                } else if (swipe != null) {
                    // A second finger makes it a chord, not a swipe.
                    swipe = null
                }
                if (widget == null) return touches.isNotEmpty() || swipe != null
                if (!live[widget.id]?.unavailable.isNullOrEmpty()) return true
                val spec = widget.slider
                when {
                    // The name row is the slider's mute: a tap there, not a drag.
                    spec != null && widget.header.contains(gridX(event.getX(index)), gridY(event.getY(index))) ->
                        onPress?.invoke(widget.id)
                    spec != null -> {
                        // Nothing moves yet: a fader moves by how far the finger
                        // goes from here, never to where it landed.
                        val start = sliderValue(widget, spec)
                        val touch = Touch(
                            widget,
                            ++drags,
                            ValuePacer(PACE_MS),
                            startValue = start,
                            startPos = along(widget, event.getX(index), event.getY(index)),
                        )
                        touches[pointer] = touch
                        dragged[widget.id] = start
                        invalidate()
                    }
                    else -> {
                        val gx = gridX(event.getX(index))
                        val gy = gridY(event.getY(index))
                        val zone = widget.zoneRects.indexOfFirst { it.contains(gx, gy) }
                        if (zone >= 0) {
                            // A button of its own: fires as it lands, and only it sinks.
                            touches[pointer] = Touch(widget, 0, null, zone = zone)
                            zoneDown[widget.id] = zone
                            invalidate()
                            onZone?.invoke(widget.id, widget.zones[zone].name)
                        } else {
                            touches[pointer] = Touch(widget, 0, null)
                            press(widget.id, true)
                            fire(recognizer.down(widget.id, widget.gestures, now))
                        }
                    }
                }
                return true
            }

            MotionEvent.ACTION_MOVE -> {
                velocity?.addMovement(event)
                val pending = swipe
                val at = pending?.let { event.findPointerIndex(it.pointer) } ?: -1
                if (pending != null && at >= 0) {
                    val dx = event.getX(at) - pending.startX
                    val dy = event.getY(at) - pending.startY
                    if (!pending.claimed && Pager.claims(dx, dy, touchSlop)) pending.claimed = true
                    if (pending.claimed) {
                        pending.dx = dx
                        slide.snap(Pager.offset(dx, pageIndex, deck?.pages?.size ?: 1))
                        invalidate()
                    }
                }
                for (index in 0 until event.pointerCount) {
                    val touch = touches[event.getPointerId(index)] ?: continue
                    if (touch.pacer != null) {
                        slide(touch, event.getX(index), event.getY(index), now, lifted = false)
                    }
                }
                return true
            }

            MotionEvent.ACTION_UP, MotionEvent.ACTION_POINTER_UP -> {
                val index = event.actionIndex
                val pointer = event.getPointerId(index)
                val pending = swipe
                if (pending != null && pending.pointer == pointer) {
                    swipe = null
                    val count = deck?.pages?.size ?: 1
                    if (pending.claimed) {
                        velocity?.addMovement(event)
                        velocity?.computeCurrentVelocity(1000)
                        val speed = velocity?.getXVelocity(pointer) ?: 0f
                        turnTo(Pager.settle(pending.dx, speed, width.toFloat(), pageIndex, count), slide.value)
                    } else if (event.getY(index) >= height - pageBarHeight()) {
                        // A tap on the page bar: straight to the nearest dot's page.
                        turnTo(pageAtDot(event.getX(index), count), 0f)
                    }
                }
                if (event.actionMasked == MotionEvent.ACTION_UP) {
                    velocity?.recycle()
                    velocity = null
                }
                val touch = touches.remove(pointer) ?: return true
                lift(touch, event.getX(index), event.getY(index), now)
                return true
            }

            // The system took the gesture away (a swipe from the edge, a
            // dialog): every finger counts as lifted, or a hold would stick.
            MotionEvent.ACTION_CANCEL -> {
                // A half-done swipe springs back to the page it started on.
                if (swipe?.claimed == true) turnTo(pageIndex, slide.value)
                swipe = null
                velocity?.recycle()
                velocity = null
                releaseAll()
                return true
            }
        }
        return super.onTouchEvent(event)
    }

    /** Every finger counts as gone: a hold ends, a key springs back, a fader lets go. */
    private fun releaseAll() {
        val now = SystemClock.uptimeMillis()
        for (touch in touches.values) {
            if (touch.zone >= 0) {
                zoneDown.remove(touch.widget.id)
                invalidate()
            } else if (touch.pacer != null) {
                lift(touch, x = null, y = null, now = now)
            } else {
                press(touch.widget.id, false)
                fire(recognizer.cancel(touch.widget.id, touch.widget.gestures))
            }
        }
        touches.clear()
    }

    private fun lift(touch: Touch, x: Float?, y: Float?, now: Long) {
        if (touch.zone >= 0) {
            zoneDown.remove(touch.widget.id)
            invalidate()
            return
        }
        if (touch.pacer != null) {
            if (x != null && y != null) {
                slide(touch, x, y, now, lifted = true)
            } else {
                val value = dragged[touch.widget.id] ?: return
                if (touch.moved) onSlide?.invoke(touch.widget.id, touch.pacer.release(value, now), touch.drag, true)
            }
            releasedAt[touch.widget.id] = now
            // Redraw once the grace is over, so the agent's value takes over.
            postInvalidateDelayed(GRACE_MS)
        } else {
            press(touch.widget.id, false)
            fire(recognizer.up(touch.widget.id, touch.widget.gestures, now))
        }
    }

    /** A warning triangle with a "!" in it, `size` from its centre to a corner. */
    private fun drawWarning(canvas: Canvas, x: Float, y: Float, size: Float) {
        warnPaint.strokeWidth = size * 0.16f
        warnPath.reset()
        warnPath.moveTo(x, y - size)
        warnPath.lineTo(x + size * 1.1f, y + size * 0.8f)
        warnPath.lineTo(x - size * 1.1f, y + size * 0.8f)
        warnPath.close()
        canvas.drawPath(warnPath, warnPaint)
        canvas.drawLine(x, y - size * 0.4f, x, y + size * 0.15f, warnPaint)
        canvas.drawPoint(x, y + size * 0.48f, warnPaint)
    }

    /** Send what the recogniser decided, and wake it again when it next has something due. */
    private fun fire(events: List<Fired>) {
        for (event in events) {
            if (event.gesture == Gesture.TAP) onPress?.invoke(event.widgetId) else onGesture?.invoke(event.widgetId, event.gesture)
        }
        removeCallbacks(tick)
        recognizer.nextDeadline()?.let { at -> postDelayed(tick, maxOf(0L, at - SystemClock.uptimeMillis())) }
    }

    /** Push a key down, or let it spring back. Local and immediate: it never waits on the agent. */
    private fun press(widgetId: String, down: Boolean) {
        val spring = travel.getOrPut(widgetId) { Spring(0f) }
        spring.target = if (down) 1f else 0f
        wake()
    }

    /** Move a slider's thumb under a finger at screen (`x`, `y`), and send what is due. */
    private fun slide(touch: Touch, x: Float, y: Float, now: Long, lifted: Boolean) {
        val widget = touch.widget
        val spec = widget.slider ?: return
        val pacer = touch.pacer ?: return
        val value = SliderMath.dragValue(
            spec,
            widget.vertical,
            touch.startValue,
            touch.startPos,
            along(widget, x, y),
            if (widget.vertical) widget.track.height() else widget.track.width(),
        )
        dragged[widget.id] = value
        if (value != touch.startValue) touch.moved = true
        invalidate()

        if (lifted) {
            if (touch.moved) onSlide?.invoke(widget.id, pacer.release(value, now), touch.drag, true)
            return
        }
        pacer.move(value, now)?.let { onSlide?.invoke(widget.id, it, touch.drag, false) }
        pacer.dueInMs(now)?.let { due ->
            postDelayed({
                // Still on the glass and still behind: send the newest.
                if (touches.values.any { it === touch }) {
                    pacer.flush(SystemClock.uptimeMillis())
                        ?.let { onSlide?.invoke(widget.id, it, touch.drag, false) }
                }
            }, due)
        }
    }

    private fun gridX(screenX: Float): Float = (screenX - originX) / scale
    private fun gridY(screenY: Float): Float = (screenY - originY) / scale

    /** Where a screen point is along a slider's direction, in grid units. */
    private fun along(widget: DrawnWidget, screenX: Float, screenY: Float): Float =
        if (widget.vertical) gridY(screenY) else gridX(screenX)

    /** Which widget is under a screen point, if any. */
    private fun widgetAt(screenX: Float, screenY: Float): DrawnWidget? {
        if (scale <= 0f) {
            return null
        }

        // Back into grid coordinates before comparing, so a scaled-down deck
        // still hits the widget that is visibly under the finger.
        val x = (screenX - originX) / scale
        val y = (screenY - originY) / scale

        // A plain loop rather than `firstOrNull { }`: a touch does not deserve to
        // allocate a closure on the way to a hit test.
        for (widget in drawn) {
            val frame = widget.frame
            if (x >= frame.left && x < frame.right && y >= frame.top && y < frame.bottom) {
                return widget
            }
        }

        return null
    }

    /** The KNORR pad: a dot every few pixels, as one tiled bitmap rather than thousands of circles. */
    private fun dotGrid(): Shader {
        val size = (22f * resources.displayMetrics.density / 1.5f).toInt().coerceAtLeast(12)
        val tile = Bitmap.createBitmap(size, size, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(tile)
        canvas.drawColor(BACKGROUND)
        canvas.drawCircle(size / 2f, size / 2f, 1.2f, Paint(Paint.ANTI_ALIAS_FLAG).apply { color = DOT })
        return BitmapShader(tile, Shader.TileMode.REPEAT, Shader.TileMode.REPEAT)
    }

    private companion object {
        const val KEY_RADIUS = 16f
        const val KEY_DROP = 4f
        const val SLIDER_INSET = 12f

        /** The page bar's height and the distance between its dots, in dp. */
        const val PAGE_BAR_DP = 28f
        const val PAGE_DOT_PITCH_DP = 18f

        /** One value to the agent per this many milliseconds while dragging. */
        const val PACE_MS = 33L

        /** How long after a finger lifts its value outranks the agent's. */
        const val GRACE_MS = 400L

        // The editor's `--fd-deck-*` tokens (packages/tokens/src/theme.css):
        // change one, change both, or the preview stops telling the truth.
        // `res/values/colors.xml` carries the window background.
        val BACKGROUND = Color.parseColor("#0A0A0B")
        val DOT = Color.parseColor("#17171A")
        val KEY_TOP = Color.parseColor("#232327")
        val KEY_BOTTOM = Color.parseColor("#17171A")
        val KEY_LINE = Color.parseColor("#2A2A2F")
        val KEY_BASE = Color.parseColor("#040405")
        val INK = Color.parseColor("#ECEBE6")
        val MUTED_INK = Color.parseColor("#8B8A85")
        val LIT_FACE = Color.parseColor("#2E1C14")
        val LIT_INK = Color.parseColor("#FFB089")
        /** The strip of zones: a shade over the face, a hairline, and a sunk button. */
        val ZONE_SHADE = Color.argb(110, 0, 0, 0)
        val ZONE_DOWN = Color.argb(90, 0, 0, 0)
        val ZONE_LINE = Color.argb(60, 255, 255, 255)
        val LIT_LINE = Color.parseColor("#6A2E16")
        val GLINT = Color.argb(18, 255, 255, 255)
        val SIGNAL = Color.parseColor("#FF5A1F")
        val NUMBER = Color.parseColor("#FF8A5C")
        val OK = Color.parseColor("#4ADE80")
        val WAIT = Color.parseColor("#FBBF24")
        /** Over a key whose plugin lost what it does. */
        val UNAVAILABLE_SHADE = Color.argb(170, 5, 5, 6)
        val TRACK = Color.parseColor("#0A0A0B")
        val LEVEL = Color.parseColor("#FF5A1F")
        val MUTED_LEVEL = Color.parseColor("#3A3A40")
        val THUMB = Color.parseColor("#DCDBD5")
        val THUMB_SHADOW = Color.parseColor("#99000000")
        val PLATE = Color.parseColor("#141416")

        /** The look a slider's channel shows while muted (see the portal's slider.lib). */
        const val MUTED_STATE = "muted"

        /** The look a "lights up while" key shows when lit (see the portal's editing.lib). */
        const val LIT_STATE = "on"
    }
}

package dev.flexdeck.app

import android.animation.ObjectAnimator
import android.app.Activity
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.ColorFilter
import android.graphics.Paint
import android.graphics.PixelFormat
import android.graphics.Rect
import android.graphics.Typeface
import android.graphics.drawable.Drawable
import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.text.Editable
import android.text.InputFilter
import android.text.InputType
import android.text.TextWatcher
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.ViewTreeObserver
import android.view.WindowInsets
import android.view.animation.OvershootInterpolator
import android.view.inputmethod.EditorInfo
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import java.net.InetSocketAddress
import java.net.Socket

/**
 * Pairing, in two steps: where the PC is, then the six-digit code it shows.
 *
 * Two panels, as the desktop's pair screen has: on the left the brand, the
 * picture of this tablet reaching the PC, and what the step is about; on the
 * right the step itself, with the dotted ground the work happens on. Side by
 * side in landscape, stacked in portrait. Only the right panel's step swaps; the
 * picture keeps running underneath.
 *
 * The code is typed with the system keyboard, pairs by itself on the last digit,
 * and shakes when the agent says no.
 *
 * Built from plain views in code, like the rest of the app: no AppCompat and no
 * layouts, on tablets with a gigabyte of RAM.
 */
class PairingScreen(
    private val activity: Activity,
    private val initialHost: String,
    private val initialPort: Int,
    /** Why we are here again, when the agent refused this tablet. */
    private val reason: String?,
    /** Back to the deck, when this is a switch from one; null on a first pairing. */
    private val back: (() -> Unit)?,
    /** Pair with the agent; call `done` with the failure's words, or `null` on success. */
    private val pair: (host: String, port: Int, pin: String, name: String, done: (String?) -> Unit) -> Unit,
) {
    private val density = activity.resources.displayMetrics.density
    private fun dp(value: Float) = (value * density).toInt()

    /** Side by side or stacked; decided from the screen's real size, see [arrange]. */
    private var wide: Boolean? = null

    private var host = initialHost
    private var port = initialPort
    private var name = "Tablet"
    private var busy = false

    private val headline = TextView(activity)
    private val lede = TextView(activity)
    private val stepper = LinearLayout(activity)
    /** Where each step's form goes; the rest of the screen stays put. */
    private val content = FrameLayout(activity)

    private val frame = LinearLayout(activity).apply { setBackgroundColor(BACKGROUND) }
    private val story = story()
    private val art = AsciiLinkView(activity)
    private val divider = View(activity).apply { setBackgroundColor(LINE) }
    private val work = work()

    /** The view to show. */
    fun view(): View {
        story.addView(art, 1)
        frame.addView(story)
        frame.addView(divider)
        frame.addView(work)
        val start = activity.resources.displayMetrics
        arrange(start.widthPixels, start.heightPixels)

        val scroll = ScrollView(activity).apply {
            isFillViewport = true
            setBackgroundColor(BACKGROUND)
            addView(frame)
        }
        // The activity handles rotation itself, and can start before the display
        // has turned: follow the size the screen actually has, not the one at launch.
        scroll.addOnLayoutChangeListener { _, left, top, right, bottom, _, _, _, _ ->
            if (arrange(right - left, bottom - top)) scroll.post { scroll.requestLayout() }
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            // The keyboard overlays the window rather than resizing it. Padding the
            // content by its height leaves room to scroll everything above it.
            scroll.setOnApplyWindowInsetsListener { _, insets ->
                val keyboard = insets.getInsets(WindowInsets.Type.ime()).bottom
                frame.setPadding(0, 0, 0, keyboard)
                // Then bring the field being typed into above the keyboard, which
                // covers half the screen in landscape. By hand: ScrollView's own
                // scroll-into-view counts the area under the keyboard as visible.
                // And after the layout the padding causes, since the panels
                // stretch with it.
                if (keyboard > 0) {
                    scroll.viewTreeObserver.addOnGlobalLayoutListener(object : ViewTreeObserver.OnGlobalLayoutListener {
                        override fun onGlobalLayout() {
                            scroll.viewTreeObserver.removeOnGlobalLayoutListener(this)
                            val focused = scroll.findFocus() ?: return
                            val box = Rect(0, 0, focused.width, focused.height)
                            scroll.offsetDescendantRectToMyCoords(focused, box)
                            val overlap = box.bottom + dp(24f) - (scroll.height - keyboard) - scroll.scrollY
                            if (overlap > 0) scroll.smoothScrollBy(0, overlap)
                        }
                    })
                }
                insets
            }
        }
        showAddress()
        return scroll
    }

    // -- the two panels -------------------------------------------------------

    /**
     * Lay the panels out side by side on a wide screen, stacked otherwise.
     * Returns whether that changed.
     */
    private fun arrange(width: Int, height: Int): Boolean {
        if (width == 0 || height == 0) return false
        val side = width > height && width / density >= 720f
        if (side == wide) return false
        wide = side
        val match = ViewGroup.LayoutParams.MATCH_PARENT
        val wrap = ViewGroup.LayoutParams.WRAP_CONTENT
        frame.orientation = if (side) LinearLayout.HORIZONTAL else LinearLayout.VERTICAL
        story.layoutParams = if (side) LinearLayout.LayoutParams(0, match, 1f) else LinearLayout.LayoutParams(match, wrap)
        divider.layoutParams = if (side) LinearLayout.LayoutParams(dp(1f), match) else LinearLayout.LayoutParams(match, dp(1f))
        work.layoutParams = if (side) LinearLayout.LayoutParams(0, match, 1f) else LinearLayout.LayoutParams(match, 0, 1f)
        art.layoutParams =
            if (side) LinearLayout.LayoutParams(match, 0, 1f).apply { topMargin = dp(24f); bottomMargin = dp(24f) }
            else LinearLayout.LayoutParams(match, dp(180f)).apply { topMargin = dp(16f); bottomMargin = dp(16f) }
        val gutter = dp(if (side) 48f else 32f)
        story.setPadding(gutter, dp(40f), gutter, dp(if (side) 48f else 28f))
        work.setPadding(gutter, dp(40f), gutter, dp(if (side) 48f else 32f))
        headline.textSize = if (side) 40f else 32f
        return true
    }

    /** The left panel: brand, the picture (added in [view]), and what this step is about. */
    private fun story() = LinearLayout(activity).apply {
        orientation = LinearLayout.VERTICAL
        addView(brand())
        headline.apply {
            setTextColor(INK)
            typeface = Typeface.create(Typeface.SANS_SERIF, Typeface.BOLD)
            setLineSpacing(0f, 1.0f)
            letterSpacing = -0.01f
        }
        addView(headline)
        lede.apply {
            textSize = 15f
            setTextColor(INK_2)
            setLineSpacing(0f, 1.3f)
            setPadding(0, dp(12f), 0, 0)
            maxWidth = dp(440f)
        }
        addView(lede)
    }

    /** The right panel: the steps across the top, and the step's form under them. */
    private fun work() = LinearLayout(activity).apply {
        orientation = LinearLayout.VERTICAL
        background = DotGrid(dp(18f).toFloat(), dp(1f).toFloat())
        stepper.apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(0, 0, 0, dp(28f))
        }
        addView(stepper)
        addView(content, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
    }

    private fun tell(title: String, body: String, step: Int) {
        headline.text = title
        lede.text = body
        steps(step)
    }

    // -- step 1: where the PC is ----------------------------------------------

    private fun showAddress() {
        tell(
            "Connect this tablet\nto your PC.",
            "Same Wi-Fi, no accounts. Pick your PC or type the address the FlexDeck app shows under Pair.",
            1,
        )
        val column = column()
        if (reason != null) {
            column.addView(
                notice("This tablet is no longer paired: $reason\nShow a new code on the PC and pair again."),
                LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { bottomMargin = dp(20f) },
            )
        }

        val address = field(written(host, port).ifEmpty { "192.168." }, InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_URI)
        // The PC this tablet last paired with, checked for an answer. There is no
        // search of the network: the agent does not announce itself, so a list
        // of "found" PCs here would be made up.
        if (initialHost.isNotEmpty()) {
            column.addView(heading("Last connected", dot = true))
            column.addView(lastPc(address))
            column.addView(heading("Or type the address"))
        } else {
            column.addView(heading("Type the address"))
        }
        column.addView(address)

        val nameField = field(name, InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_CAP_WORDS).apply { textSize = 16f }
        column.addView(heading("Name this tablet"))
        column.addView(nameField)

        val error = TextView(activity).apply {
            textSize = 14f
            setTextColor(BAD)
            setPadding(0, dp(12f), 0, 0)
        }
        column.addView(error)
        column.addView(View(activity), LinearLayout.LayoutParams(1, 0, 1f))
        val actions = LinearLayout(activity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        if (back != null) {
            actions.addView(
                key("←  Back to deck", signal = false) {
                    hideKeyboard(address)
                    back.invoke()
                },
                LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, dp(52f)),
            )
        }
        actions.addView(View(activity), LinearLayout.LayoutParams(0, 1, 1f))
        column.addView(actions, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { topMargin = dp(24f) })
        actions.addView(
            key("Next", signal = true) {
                val typed = address.text.toString().trim()
                val (typedHost, typedPort) = parse(typed)
                if (typedHost.isEmpty() || typed == "192.168.") {
                    error.text = "Type the PC's address first."
                    shake(address)
                    return@key
                }
                host = typedHost
                port = typedPort
                name = nameField.text.toString().trim().ifEmpty { "Tablet" }
                hideKeyboard(address)
                showCode()
            },
            LinearLayout.LayoutParams(dp(132f), dp(52f)),
        )
        swap(column)
    }

    /** The PC this tablet last knew, with a dot for whether it answers right now. */
    private fun lastPc(address: EditText): View {
        val dot = View(activity).apply {
            background = GradientDrawable().apply { shape = GradientDrawable.OVAL; setColor(MUTED) }
        }
        val state = mono("CHECKING", MUTED)
        val card = LinearLayout(activity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(dp(14f), dp(12f), dp(16f), dp(12f))
            background = GradientDrawable().apply {
                cornerRadius = dp(12f).toFloat()
                setColor(CARD)
                setStroke(dp(1f), LINE)
            }
            addView(
                TextView(activity).apply {
                    text = "[_]"
                    typeface = Typeface.MONOSPACE
                    textSize = 12f
                    gravity = Gravity.CENTER
                    setTextColor(INK_2)
                    background = GradientDrawable().apply {
                        cornerRadius = dp(8f).toFloat()
                        setColor(WELL)
                        setStroke(dp(1f), LINE)
                    }
                },
                LinearLayout.LayoutParams(dp(40f), dp(40f)),
            )
            addView(
                LinearLayout(activity).apply {
                    orientation = LinearLayout.VERTICAL
                    setPadding(dp(14f), 0, dp(12f), 0)
                    addView(TextView(activity).apply {
                        text = "Your PC"
                        textSize = 15f
                        setTextColor(INK)
                        typeface = Typeface.create(Typeface.SANS_SERIF, Typeface.BOLD)
                    })
                    addView(mono("$initialHost · $initialPort", MUTED).apply { setPadding(0, dp(4f), 0, 0) })
                },
                LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
            )
            addView(state.apply { setPadding(0, 0, dp(10f), 0) })
            addView(dot, LinearLayout.LayoutParams(dp(8f), dp(8f)))
            isClickable = true
            setOnClickListener {
                address.setText(written(initialHost, initialPort))
                address.setSelection(address.text.length)
                (background as GradientDrawable).setStroke(dp(1f), SIGNAL)
            }
        }

        // A plain TCP connect: enough to say the agent is listening, and nothing
        // is sent. Off the main thread; `post` holds the answer until the card is
        // on screen, which a quick answer can beat.
        Thread {
            val up = try {
                Socket().use { it.connect(InetSocketAddress(initialHost, initialPort), PROBE_MS); true }
            } catch (_: Exception) {
                false
            }
            card.post {
                (dot.background as GradientDrawable).setColor(if (up) OK else BAD)
                state.text = if (up) "ONLINE" else "NOT ANSWERING"
                state.setTextColor(if (up) OK else MUTED)
            }
        }.apply { isDaemon = true }.start()
        return card
    }

    // -- step 2: the code -----------------------------------------------------

    private fun showCode() {
        tell(
            "Type the code\nfrom your PC.",
            "The FlexDeck app shows $PIN_LENGTH digits under Pair. They work once, for two minutes.",
            2,
        )
        val column = column()
        column.addView(heading("Code for ${written(host, port)}", dot = true))

        // The system keyboard, in its number layout: paste, a keyboard case and
        // TalkBack all work without anything of ours in between.
        val code = field("", InputType.TYPE_CLASS_NUMBER).apply {
            filters = arrayOf(InputFilter.LengthFilter(PIN_LENGTH))
            gravity = Gravity.CENTER
            textSize = 40f
            letterSpacing = 0.4f
            setTextColor(DIGIT)
            setShadowLayer(dp(10f).toFloat(), 0f, 0f, SIGNAL)
            hint = "•".repeat(PIN_LENGTH)
            setHintTextColor(LINE)
            // A fullscreen landscape keyboard would hide the screen it types into.
            imeOptions = EditorInfo.IME_ACTION_GO or EditorInfo.IME_FLAG_NO_EXTRACT_UI or EditorInfo.IME_FLAG_NO_FULLSCREEN
        }
        column.addView(code, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))

        val status = mono("", MUTED).apply {
            gravity = Gravity.CENTER
            setPadding(0, dp(16f), 0, 0)
        }
        column.addView(status, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))

        fun submit() {
            val pin = code.text.toString()
            if (busy || pin.length != PIN_LENGTH) return
            busy = true
            code.isEnabled = false
            status.setTextColor(MUTED)
            status.text = "PAIRING WITH $host…"
            pair(host, port, pin, name) { failure ->
                busy = false
                if (failure == null) {
                    status.setTextColor(OK)
                    status.text = "PAIRED"
                    code.setTextColor(OK)
                    hideKeyboard(code)
                } else {
                    // The agent says why — wrong code, expired, too many tries —
                    // and that is worth showing as it is.
                    status.setTextColor(BAD)
                    status.text = failure
                    code.isEnabled = true
                    code.setText("")
                    shake(code)
                    code.requestFocus()
                }
            }
        }

        code.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(text: CharSequence?, start: Int, count: Int, after: Int) {}
            override fun onTextChanged(text: CharSequence?, start: Int, before: Int, count: Int) {}
            override fun afterTextChanged(text: Editable?) {
                if (!busy && !text.isNullOrEmpty()) status.text = ""
                if (text?.length == PIN_LENGTH) submit()
            }
        })
        code.setOnEditorActionListener { _, _, _ -> submit(); true }

        column.addView(View(activity), LinearLayout.LayoutParams(1, 0, 1f))
        column.addView(
            key("←  Different PC", signal = false) {
                if (busy) return@key
                hideKeyboard(code)
                showAddress()
            },
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, dp(52f)).apply { topMargin = dp(24f); gravity = Gravity.START },
        )
        swap(column)

        code.requestFocus()
        code.post {
            val manager = activity.getSystemService(Activity.INPUT_METHOD_SERVICE) as android.view.inputmethod.InputMethodManager
            manager.showSoftInput(code, android.view.inputmethod.InputMethodManager.SHOW_IMPLICIT)
        }
    }

    // -- pieces ---------------------------------------------------------------

    /** Slide the new step in from the right, and the old one out. */
    private fun swap(next: View) {
        val old = content.getChildAt(0)
        content.addView(next, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        if (old == null) return
        next.alpha = 0f
        next.translationX = dp(40f).toFloat()
        next.animate().alpha(1f).translationX(0f).setDuration(320).setInterpolator(OvershootInterpolator(0.6f)).start()
        old.animate().alpha(0f).translationX(-dp(30f).toFloat()).setDuration(180).withEndAction { content.removeView(old) }.start()
    }

    private fun column() = LinearLayout(activity).apply { orientation = LinearLayout.VERTICAL }

    /** The animated [f], the word, and the version, above the picture. */
    private fun brand() = LinearLayout(activity).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        addView(MarkView(activity), LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, dp(22f)))
        addView(mono("FLEXDECK", INK).apply { setPadding(dp(10f), 0, 0, 0); letterSpacing = 0.3f })
        version()?.let { addView(mono("v$it", FAINT).apply { setPadding(dp(14f), 0, 0, 0) }) }
    }

    private fun version(): String? = try {
        activity.packageManager.getPackageInfo(activity.packageName, 0).versionName
    } catch (_: Exception) {
        null
    }

    /** Step one on the left, step two on the right, as the desktop draws them. */
    private fun steps(current: Int) {
        stepper.removeAllViews()
        listOf("ADDRESS", "CODE").forEachIndexed { index, word ->
            val step = index + 1
            val on = step == current
            val done = step < current
            if (index > 0) stepper.addView(View(activity), LinearLayout.LayoutParams(0, 1, 1f))
            stepper.addView(
                TextView(activity).apply {
                    text = if (done) "✓" else "0$step"
                    typeface = Typeface.MONOSPACE
                    textSize = 10f
                    gravity = Gravity.CENTER
                    setTextColor(if (on) BACKGROUND else if (done) OK else MUTED)
                    background = GradientDrawable().apply {
                        shape = GradientDrawable.OVAL
                        setColor(if (on) INK else Color.TRANSPARENT)
                        setStroke(dp(1f), if (on) INK else if (done) OK else LINE)
                    }
                },
                LinearLayout.LayoutParams(dp(26f), dp(26f)),
            )
            stepper.addView(mono(word, if (on) INK else MUTED).apply { setPadding(dp(10f), 0, 0, 0) })
        }
    }

    /** A small caps label over a field; with a signal dot when it heads a section. */
    private fun heading(text: String, dot: Boolean = false) = LinearLayout(activity).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        setPadding(0, dp(18f), 0, dp(10f))
        if (dot) {
            addView(
                View(activity).apply { background = GradientDrawable().apply { shape = GradientDrawable.OVAL; setColor(AMBER) } },
                LinearLayout.LayoutParams(dp(6f), dp(6f)).apply { marginEnd = dp(10f) },
            )
        }
        addView(mono(text.uppercase(), MUTED))
    }

    private fun notice(text: String) = TextView(activity).apply {
        this.text = text
        textSize = 14f
        setTextColor(BAD)
        setLineSpacing(0f, 1.3f)
        background = GradientDrawable().apply {
            cornerRadius = dp(12f).toFloat()
            setColor(BAD_SOFT)
            setStroke(dp(1f), BAD_LINE)
        }
        setPadding(dp(14f), dp(12f), dp(14f), dp(12f))
    }

    private fun mono(text: String, color: Int) = TextView(activity).apply {
        this.text = text
        typeface = Typeface.MONOSPACE
        textSize = 10f
        letterSpacing = 0.2f
        setTextColor(color)
    }

    private fun field(initial: String, type: Int) = EditText(activity).apply {
        setText(initial)
        inputType = type
        setSingleLine(true)
        typeface = Typeface.MONOSPACE
        textSize = 22f
        setTextColor(INK)
        setHintTextColor(MUTED)
        setPadding(dp(18f), dp(16f), dp(18f), dp(16f))
        background = GradientDrawable().apply {
            cornerRadius = dp(12f).toFloat()
            setColor(WELL)
            setStroke(dp(1f), LINE)
        }
        setOnFocusChangeListener { _, focused -> (background as GradientDrawable).setStroke(dp(if (focused) 2f else 1f), if (focused) SIGNAL else LINE) }
    }

    /** A key like the deck's: a face with a top edge, pressed down while touched. */
    private fun key(text: String, signal: Boolean, onTap: () -> Unit) = TextView(activity).apply {
        this.text = text
        gravity = Gravity.CENTER
        textSize = 15f
        typeface = Typeface.create(Typeface.SANS_SERIF, Typeface.BOLD)
        setTextColor(if (signal) SIGNAL_INK else INK_2)
        setPadding(dp(22f), 0, dp(22f), 0)
        background = GradientDrawable(
            GradientDrawable.Orientation.TOP_BOTTOM,
            if (signal) intArrayOf(SIGNAL_HI, SIGNAL) else intArrayOf(KEY_TOP, KEY_BOTTOM),
        ).apply {
            cornerRadius = dp(12f).toFloat()
            setStroke(dp(1f), if (signal) SIGNAL_HI else LINE)
        }
        elevation = dp(if (signal) 10f else 4f).toFloat()
        if (signal && Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            outlineSpotShadowColor = SIGNAL
            outlineAmbientShadowColor = SIGNAL
        }
        isClickable = true
        setOnTouchListener { view, event ->
            when (event.actionMasked) {
                MotionEvent.ACTION_DOWN -> view.animate().translationY(dp(3f).toFloat()).scaleX(0.97f).scaleY(0.97f).setDuration(70).start()
                MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL ->
                    view.animate().translationY(0f).scaleX(1f).scaleY(1f).setDuration(260).setInterpolator(OvershootInterpolator(3f)).start()
            }
            if (event.actionMasked == MotionEvent.ACTION_UP) view.performClick()
            true
        }
        setOnClickListener { onTap() }
    }

    private fun shake(view: View) {
        val d = dp(10f).toFloat()
        ObjectAnimator.ofFloat(view, View.TRANSLATION_X, 0f, -d, d, -d * 0.8f, d * 0.8f, -d * 0.4f, d * 0.4f, 0f).setDuration(420).start()
    }

    private fun hideKeyboard(view: View) {
        val manager = activity.getSystemService(Activity.INPUT_METHOD_SERVICE) as android.view.inputmethod.InputMethodManager
        manager.hideSoftInputFromWindow(view.windowToken, 0)
    }

    /** An address as typed: the port only when it is not the usual one. */
    private fun written(host: String, port: Int) = when {
        host.isEmpty() -> ""
        port == DEFAULT_PORT -> host
        else -> "$host:$port"
    }

    /** `host` or `host:port`, back into its parts. */
    private fun parse(typed: String): Pair<String, Int> {
        val colon = typed.lastIndexOf(':')
        if (colon > 0) {
            val p = typed.substring(colon + 1).toIntOrNull()
            if (p != null && p in 1..65535) return typed.substring(0, colon) to p
        }
        return typed to DEFAULT_PORT
    }

    /** The right panel's ground: a faint dot every few dp. */
    private class DotGrid(private val step: Float, private val radius: Float) : Drawable() {
        private val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = DOT }

        override fun draw(canvas: Canvas) {
            canvas.drawColor(PANEL)
            val b = bounds
            var y = b.top + step / 2
            while (y < b.bottom) {
                var x = b.left + step / 2
                while (x < b.right) {
                    canvas.drawCircle(x, y, radius, paint)
                    x += step
                }
                y += step
            }
        }

        override fun setAlpha(alpha: Int) {
            paint.alpha = alpha
        }

        override fun setColorFilter(colorFilter: ColorFilter?) {
            paint.colorFilter = colorFilter
        }

        @Deprecated("Deprecated in Java")
        override fun getOpacity() = PixelFormat.OPAQUE
    }

    private companion object {
        /** Mirrors the tablet PIN in crates/flexdeck-pairing/src/session.rs. */
        const val PIN_LENGTH = 6
        /** Mirrors MainActivity's default; written addresses leave it out. */
        const val DEFAULT_PORT = 23539
        const val PROBE_MS = 1500

        val BACKGROUND = Color.parseColor("#0A0A0B")
        val PANEL = Color.parseColor("#0C0C0E")
        val DOT = Color.parseColor("#1A1A1E")
        val CARD = Color.parseColor("#131316")
        val WELL = Color.parseColor("#070708")
        val LINE = Color.parseColor("#2A2A2F")
        val KEY_TOP = Color.parseColor("#26262B")
        val KEY_BOTTOM = Color.parseColor("#18181B")
        val INK = Color.parseColor("#ECEBE6")
        val INK_2 = Color.parseColor("#A9A8A2")
        val MUTED = Color.parseColor("#8B8A85")
        val FAINT = Color.parseColor("#55545A")
        val SIGNAL = Color.parseColor("#FF5A1F")
        val SIGNAL_HI = Color.parseColor("#FF7A45")
        val SIGNAL_INK = Color.parseColor("#140702")
        val AMBER = Color.parseColor("#F5A524")
        val DIGIT = Color.parseColor("#FFB08F")
        val OK = Color.parseColor("#4ADE80")
        val BAD = Color.parseColor("#F87171")
        val BAD_SOFT = Color.parseColor("#2A1414")
        val BAD_LINE = Color.parseColor("#5A2626")
    }
}

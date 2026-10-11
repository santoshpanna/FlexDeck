package dev.flexdeck.app

import android.animation.ObjectAnimator
import android.app.Activity
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.text.InputType
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.WindowInsets
import android.view.animation.OvershootInterpolator
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.GridLayout
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView

/**
 * Pairing, in two steps on one screen: where the PC is, then the four-digit
 * code it shows. The code is typed on an on-screen keypad of keys like the
 * deck's own, fills four cells as it goes, pairs by itself on the fourth digit,
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
    /** Pair with the agent; call `done` with the failure's words, or `null` on success. */
    private val pair: (host: String, port: Int, pin: String, name: String, done: (String?) -> Unit) -> Unit,
) {
    private val density = activity.resources.displayMetrics.density
    private fun dp(value: Float) = (value * density).toInt()

    private val content = FrameLayout(activity)
    private var host = initialHost
    private var port = initialPort
    private var name = "Tablet"
    private var pin = ""
    private var busy = false

    /** The view to show. */
    fun view(): View {
        val scroll = ScrollView(activity).apply {
            isFillViewport = true
            setBackgroundColor(BACKGROUND)
            addView(content)
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            // The keyboard overlays the window rather than resizing it; padding by
            // its height keeps the address field and Next reachable.
            scroll.setOnApplyWindowInsetsListener { view, insets ->
                view.setPadding(0, 0, 0, insets.getInsets(WindowInsets.Type.ime()).bottom)
                insets
            }
        }
        showAddress()
        return scroll
    }

    // -- step 1: where the PC is ----------------------------------------------

    private fun showAddress() {
        val column = column()
        column.addView(brand())
        column.addView(steps(1))
        column.addView(title("Connect this tablet\nto your PC."))
        if (reason != null) {
            column.addView(notice("This tablet is no longer paired: $reason\nShow a new code on the PC and pair again."))
        }
        column.addView(body("Same Wi-Fi, no accounts. Type the address the FlexDeck app shows under Pair."))

        val address = field(host.ifEmpty { "192.168." }, InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_URI)
        val portField = field(port.toString(), InputType.TYPE_CLASS_NUMBER)
        val nameField = field(name, InputType.TYPE_CLASS_TEXT)
        column.addView(label("Address"))
        column.addView(address)
        val row = LinearLayout(activity).apply { orientation = LinearLayout.HORIZONTAL }
        row.addView(
            LinearLayout(activity).apply {
                orientation = LinearLayout.VERTICAL
                addView(label("Port"))
                addView(portField)
            },
            LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f).apply { marginEnd = dp(12f) },
        )
        row.addView(
            LinearLayout(activity).apply {
                orientation = LinearLayout.VERTICAL
                addView(label("Name this tablet"))
                addView(nameField)
            },
            LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 2f),
        )
        column.addView(row)

        val error = body("").apply { setTextColor(BAD) }
        column.addView(error)
        column.addView(
            key("Next  →", signal = true) {
                val typed = address.text.toString().trim()
                if (typed.isEmpty() || typed == "192.168.") {
                    error.text = "Type the PC's address first."
                    shake(address)
                    return@key
                }
                host = typed
                port = portField.text.toString().trim().toIntOrNull() ?: initialPort
                name = nameField.text.toString().trim().ifEmpty { "Tablet" }
                hideKeyboard(address)
                showCode()
            },
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(56f)).apply { topMargin = dp(8f) },
        )
        swap(column)
    }

    // -- step 2: the code -----------------------------------------------------

    private fun showCode() {
        pin = ""
        val column = column()
        column.addView(brand())
        column.addView(steps(2))
        column.addView(title("Type the code\nfrom your PC."))
        column.addView(body("The FlexDeck app shows four digits under Pair. They work once, for five minutes."))

        val cells = List(4) { cell() }
        val cellRow = LinearLayout(activity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER
            cells.forEach { addView(it, LinearLayout.LayoutParams(dp(72f), dp(92f)).apply { marginStart = dp(6f); marginEnd = dp(6f) }) }
        }
        column.addView(cellRow, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { topMargin = dp(8f); bottomMargin = dp(8f) })

        val status = body("").apply { gravity = Gravity.CENTER }
        column.addView(status)

        fun render() {
            cells.forEachIndexed { index, cell ->
                val digit = pin.getOrNull(index)?.toString() ?: ""
                if (cell.text.toString() != digit) {
                    cell.text = digit
                    if (digit.isNotEmpty()) {
                        cell.scaleX = 0.6f
                        cell.scaleY = 0.6f
                        cell.animate().scaleX(1f).scaleY(1f).setDuration(220).setInterpolator(OvershootInterpolator(2.5f)).start()
                    }
                }
                (cell.background as GradientDrawable).setStroke(dp(1.5f), if (index == pin.length && !busy) SIGNAL else LINE)
            }
        }

        fun submit() {
            busy = true
            render()
            status.setTextColor(MUTED)
            status.text = "PAIRING WITH $host…"
            pair(host, port, pin, name) { failure ->
                busy = false
                if (failure == null) {
                    status.setTextColor(OK)
                    status.text = "PAIRED"
                    cells.forEach { it.setTextColor(OK) }
                } else {
                    // The agent says why — wrong code, expired, too many tries —
                    // and that is worth showing as it is.
                    status.setTextColor(BAD)
                    status.text = failure
                    shake(cellRow)
                    pin = ""
                    render()
                }
            }
        }

        val pad = GridLayout(activity).apply {
            columnCount = 3
            rowCount = 4
        }
        val labels = listOf("1", "2", "3", "4", "5", "6", "7", "8", "9", "⌫", "0", "CLR")
        labels.forEach { text ->
            val button = key(text, signal = false, mono = true) {
                if (busy) return@key
                when (text) {
                    "⌫" -> pin = pin.dropLast(1)
                    "CLR" -> pin = ""
                    else -> if (pin.length < 4) pin += text
                }
                status.text = ""
                render()
                if (pin.length == 4) submit()
            }
            pad.addView(
                button,
                GridLayout.LayoutParams(GridLayout.spec(GridLayout.UNDEFINED, 1f), GridLayout.spec(GridLayout.UNDEFINED, 1f)).apply {
                    width = 0
                    height = dp(64f)
                    setMargins(dp(5f), dp(5f), dp(5f), dp(5f))
                },
            )
        }
        column.addView(pad, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))

        column.addView(
            TextView(activity).apply {
                text = "Different PC"
                setTextColor(MUTED)
                textSize = 14f
                gravity = Gravity.CENTER
                setPadding(0, dp(18f), 0, dp(8f))
                setOnClickListener { if (!busy) showAddress() }
            },
        )
        render()
        swap(column)
    }

    // -- pieces ---------------------------------------------------------------

    /** Slide the new step in from the right, and the old one out. */
    private fun swap(next: View) {
        val old = content.getChildAt(0)
        content.addView(next, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT, Gravity.CENTER))
        if (old == null) return
        next.alpha = 0f
        next.translationX = dp(40f).toFloat()
        next.animate().alpha(1f).translationX(0f).setDuration(320).setInterpolator(OvershootInterpolator(0.6f)).start()
        old.animate().alpha(0f).translationX(-dp(30f).toFloat()).setDuration(180).withEndAction { content.removeView(old) }.start()
    }

    private fun column() = LinearLayout(activity).apply {
        orientation = LinearLayout.VERTICAL
        val side = dp(32f)
        setPadding(side, dp(40f), side, dp(32f))
        layoutParams = FrameLayout.LayoutParams(dp(520f).coerceAtMost(activity.resources.displayMetrics.widthPixels), ViewGroup.LayoutParams.WRAP_CONTENT, Gravity.CENTER)
    }

    /** The animated [f] and the word, above the steps. */
    private fun brand() = LinearLayout(activity).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        setPadding(0, 0, 0, dp(28f))
        addView(MarkView(activity), LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, dp(22f)))
        addView(mono("FLEXDECK", INK).apply { setPadding(dp(10f), 0, 0, 0) })
    }

    private fun steps(current: Int) = LinearLayout(activity).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        setPadding(0, 0, 0, dp(24f))
        listOf("ADDRESS", "CODE").forEachIndexed { index, word ->
            val step = index + 1
            val on = step == current
            val done = step < current
            addView(
                TextView(activity).apply {
                    text = if (done) "✓" else "0$step"
                    typeface = Typeface.MONOSPACE
                    textSize = 11f
                    gravity = Gravity.CENTER
                    setTextColor(if (on) BACKGROUND else if (done) OK else MUTED)
                    background = GradientDrawable().apply {
                        shape = GradientDrawable.OVAL
                        setColor(if (on) INK else Color.TRANSPARENT)
                        setStroke(dp(1f), if (on) INK else if (done) OK else LINE)
                    }
                },
                LinearLayout.LayoutParams(dp(28f), dp(28f)),
            )
            addView(mono(word, if (on) INK else MUTED).apply { setPadding(dp(10f), 0, dp(16f), 0) })
        }
    }

    private fun title(text: String) = TextView(activity).apply {
        this.text = text
        textSize = 34f
        setTextColor(INK)
        typeface = Typeface.create(Typeface.SANS_SERIF, Typeface.BOLD)
        setLineSpacing(0f, 1.02f)
    }

    private fun body(text: String) = TextView(activity).apply {
        this.text = text
        textSize = 15f
        setTextColor(INK_2)
        setLineSpacing(0f, 1.3f)
        setPadding(0, dp(12f), 0, dp(12f))
    }

    private fun notice(text: String) = body(text).apply {
        setTextColor(BAD)
        background = GradientDrawable().apply {
            cornerRadius = dp(12f).toFloat()
            setColor(BAD_SOFT)
            setStroke(dp(1f), BAD_LINE)
        }
        setPadding(dp(14f), dp(12f), dp(14f), dp(12f))
    }

    private fun label(text: String) = mono(text.uppercase(), MUTED).apply { setPadding(0, dp(14f), 0, dp(8f)) }

    private fun mono(text: String, color: Int) = TextView(activity).apply {
        this.text = text
        typeface = Typeface.MONOSPACE
        textSize = 11f
        letterSpacing = 0.2f
        setTextColor(color)
    }

    private fun field(initial: String, type: Int) = EditText(activity).apply {
        setText(initial)
        inputType = type
        setSingleLine(true)
        typeface = Typeface.MONOSPACE
        textSize = 20f
        setTextColor(INK)
        setHintTextColor(MUTED)
        setPadding(dp(16f), dp(14f), dp(16f), dp(14f))
        background = GradientDrawable().apply {
            cornerRadius = dp(14f).toFloat()
            setColor(WELL)
            setStroke(dp(1f), LINE)
        }
        setOnFocusChangeListener { _, focused -> (background as GradientDrawable).setStroke(dp(if (focused) 2f else 1f), if (focused) SIGNAL else LINE) }
    }

    /** A recessed cell for one digit of the code. */
    private fun cell() = TextView(activity).apply {
        gravity = Gravity.CENTER
        typeface = Typeface.MONOSPACE
        textSize = 42f
        setTextColor(DIGIT)
        setShadowLayer(dp(10f).toFloat(), 0f, 0f, SIGNAL)
        background = GradientDrawable().apply {
            cornerRadius = dp(14f).toFloat()
            setColor(WELL)
            setStroke(dp(1.5f), LINE)
        }
    }

    /** A key like the deck's: a face with a top edge, pressed down while touched. */
    private fun key(text: String, signal: Boolean, mono: Boolean = false, onTap: () -> Unit) = TextView(activity).apply {
        this.text = text
        gravity = Gravity.CENTER
        textSize = if (mono) 24f else 17f
        typeface = if (mono) Typeface.MONOSPACE else Typeface.create(Typeface.SANS_SERIF, Typeface.BOLD)
        setTextColor(if (signal) SIGNAL_INK else INK)
        background = GradientDrawable(
            GradientDrawable.Orientation.TOP_BOTTOM,
            if (signal) intArrayOf(SIGNAL_HI, SIGNAL) else intArrayOf(KEY_TOP, KEY_BOTTOM),
        ).apply {
            cornerRadius = dp(16f).toFloat()
            setStroke(dp(1f), if (signal) SIGNAL_HI else LINE)
        }
        elevation = dp(4f).toFloat()
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

    private companion object {
        val BACKGROUND = Color.parseColor("#0A0A0B")
        val WELL = Color.parseColor("#070708")
        val LINE = Color.parseColor("#2A2A2F")
        val KEY_TOP = Color.parseColor("#26262B")
        val KEY_BOTTOM = Color.parseColor("#18181B")
        val INK = Color.parseColor("#ECEBE6")
        val INK_2 = Color.parseColor("#A9A8A2")
        val MUTED = Color.parseColor("#8B8A85")
        val SIGNAL = Color.parseColor("#FF5A1F")
        val SIGNAL_HI = Color.parseColor("#FF7A45")
        val SIGNAL_INK = Color.parseColor("#140702")
        val DIGIT = Color.parseColor("#FFB08F")
        val OK = Color.parseColor("#4ADE80")
        val BAD = Color.parseColor("#F87171")
        val BAD_SOFT = Color.parseColor("#2A1414")
        val BAD_LINE = Color.parseColor("#5A2626")
    }
}

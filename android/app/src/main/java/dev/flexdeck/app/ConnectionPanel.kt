package dev.flexdeck.app

import android.content.Context
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.animation.OvershootInterpolator
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView

/**
 * The connection panel, dropped from the strip's address chip: which PC this
 * tablet is on, and the two ways off it.
 *
 * - **Switch PC** opens pairing with a way back. The current pairing stays
 *   until a new one succeeds, so changing your mind costs nothing.
 * - **Disconnect** forgets this PC. That is the one that loses something, so it
 *   asks for a second tap; a deck is pressed by habit, and one stray touch
 *   should not unpair it.
 *
 * Tapping outside the card closes it.
 */
class ConnectionPanel(
    context: Context,
    host: String,
    port: Int,
    online: Boolean,
    private val onSwitch: () -> Unit,
    private val onDisconnect: () -> Unit,
) : FrameLayout(context) {
    private val density = resources.displayMetrics.density
    private fun dp(value: Float) = (value * density).toInt()

    /** Called once the panel has gone, so the owner can drop it. */
    var onClosed: (() -> Unit)? = null

    private val card = LinearLayout(context)
    private var armed = false

    init {
        setBackgroundColor(SCRIM)
        isClickable = true
        setOnClickListener { close() }

        card.apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(20f), dp(18f), dp(20f), dp(20f))
            background = GradientDrawable().apply {
                cornerRadius = dp(16f).toFloat()
                setColor(CARD)
                setStroke(dp(1f), LINE)
            }
            elevation = dp(16f).toFloat()
            // Taps on the card stay on the card.
            isClickable = true
        }

        card.addView(mono("CONNECTED TO", MUTED))
        card.addView(
            LinearLayout(context).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                setPadding(0, dp(10f), 0, dp(4f))
                addView(
                    View(context).apply {
                        background = GradientDrawable().apply { shape = GradientDrawable.OVAL; setColor(if (online) OK else WAIT) }
                    },
                    LinearLayout.LayoutParams(dp(8f), dp(8f)).apply { marginEnd = dp(10f) },
                )
                addView(TextView(context).apply {
                    text = host
                    typeface = Typeface.MONOSPACE
                    textSize = 20f
                    setTextColor(INK)
                })
            },
        )
        card.addView(mono("PORT $port · ${if (online) "ONLINE" else "NOT CONNECTED"}", MUTED).apply { setPadding(dp(18f), 0, 0, dp(18f)) })

        card.addView(
            button("⇄  Switch PC", danger = false) { onSwitch() },
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(48f)),
        )
        val disconnect = button("Disconnect", danger = true) {}
        disconnect.setOnClickListener {
            if (!armed) {
                armed = true
                disconnect.text = "Tap again to forget this PC"
                // Disarm by itself, so the next visit starts safe.
                disconnect.postDelayed({
                    armed = false
                    disconnect.text = "Disconnect"
                }, 3000)
            } else {
                onDisconnect()
            }
        }
        card.addView(disconnect, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(48f)).apply { topMargin = dp(10f) })
        card.addView(
            TextView(context).apply {
                text = "Disconnecting forgets this PC on the tablet. To remove the tablet from the PC too, revoke it in the FlexDeck app."
                textSize = 12f
                setTextColor(MUTED)
                setLineSpacing(0f, 1.25f)
                setPadding(0, dp(14f), 0, 0)
            },
        )

        addView(
            card,
            LayoutParams(dp(340f), ViewGroup.LayoutParams.WRAP_CONTENT, Gravity.TOP or Gravity.END).apply {
                topMargin = dp(44f)
                marginEnd = dp(12f)
            },
        )

        // Drop in from the chip.
        alpha = 0f
        card.translationY = -dp(12f).toFloat()
        animate().alpha(1f).setDuration(160).start()
        card.animate().translationY(0f).setDuration(260).setInterpolator(OvershootInterpolator(1.2f)).start()
    }

    fun close() {
        animate().alpha(0f).setDuration(140).withEndAction {
            (parent as? ViewGroup)?.removeView(this)
            onClosed?.invoke()
        }.start()
    }

    private fun mono(text: String, color: Int) = TextView(context).apply {
        this.text = text
        typeface = Typeface.MONOSPACE
        textSize = 10f
        letterSpacing = 0.2f
        setTextColor(color)
    }

    private fun button(text: String, danger: Boolean, onTap: () -> Unit) = TextView(context).apply {
        this.text = text
        gravity = Gravity.CENTER
        textSize = 15f
        typeface = Typeface.create(Typeface.SANS_SERIF, Typeface.BOLD)
        setTextColor(if (danger) BAD else INK)
        background = GradientDrawable(GradientDrawable.Orientation.TOP_BOTTOM, if (danger) intArrayOf(BAD_SOFT, BAD_SOFT) else intArrayOf(KEY_TOP, KEY_BOTTOM)).apply {
            cornerRadius = dp(12f).toFloat()
            setStroke(dp(1f), if (danger) BAD_LINE else LINE)
        }
        isClickable = true
        setOnTouchListener { view, event ->
            when (event.actionMasked) {
                MotionEvent.ACTION_DOWN -> view.animate().scaleX(0.97f).scaleY(0.97f).setDuration(70).start()
                MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL ->
                    view.animate().scaleX(1f).scaleY(1f).setDuration(220).setInterpolator(OvershootInterpolator(3f)).start()
            }
            false
        }
        setOnClickListener { onTap() }
    }

    private companion object {
        val SCRIM = Color.parseColor("#99000000")
        val CARD = Color.parseColor("#131316")
        val LINE = Color.parseColor("#2A2A2F")
        val KEY_TOP = Color.parseColor("#26262B")
        val KEY_BOTTOM = Color.parseColor("#18181B")
        val INK = Color.parseColor("#ECEBE6")
        val MUTED = Color.parseColor("#8B8A85")
        val OK = Color.parseColor("#4ADE80")
        val WAIT = Color.parseColor("#FBBF24")
        val BAD = Color.parseColor("#F87171")
        val BAD_SOFT = Color.parseColor("#2A1414")
        val BAD_LINE = Color.parseColor("#5A2626")
    }
}

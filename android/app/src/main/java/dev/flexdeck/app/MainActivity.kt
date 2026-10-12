package dev.flexdeck.app

import android.app.Activity
import android.content.Context
import android.content.SharedPreferences
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.view.WindowManager
import android.widget.FrameLayout
import android.widget.LinearLayout
import dev.flexdeck.client.Backoff
import dev.flexdeck.client.DeckReader
import dev.flexdeck.client.DeckSession
import dev.flexdeck.client.ServerMessage

/**
 * The whole tablet UI: pair once, then render the deck full screen.
 *
 * There is deliberately no navigation, no settings screen and no chrome. This is
 * meant to be a panel screwed to a wall, not an app someone browses.
 */
class MainActivity : Activity(), AgentClient.Listener {

    private lateinit var preferences: SharedPreferences
    private lateinit var client: AgentClient

    /** The icon set the editor offers, read once from the bundled asset. */
    private val bundledIcons: Map<String, List<String>> by lazy {
        runCatching { assets.open("icons.json").bufferedReader().use { dev.flexdeck.client.Icons.read(it.readText()) } }
            .getOrDefault(emptyMap())
    }
    private val session = DeckSession()
    private val backoff = Backoff()
    private val main = Handler(Looper.getMainLooper())

    private var deckView: DeckView? = null
    private var strip: StatusStrip? = null
    private var reconnectPending = false

    /**
     * Set once the agent has refused us, or its certificate has changed.
     *
     * Written from OkHttp's callback thread, so it is volatile. Once set, the
     * reconnect loop is dead on purpose: the pairing form is showing, and a
     * retry would only replace it.
     */
    @Volatile
    private var terminal = false

    /** Whether the activity is started, i.e. at least partly on screen. */
    private var visible = false

    /**
     * Whether the socket is up.
     *
     * Only ever changed on the main thread: every listener callback hops there
     * first.
     */
    private var connected = false

    /** Whether this activity is currently holding the display awake. */
    private var screenOn = false

    /**
     * Lets the display sleep after a drop.
     *
     * Held as a field so the same runnable can be cancelled, because a blip and
     * a real outage start identically and only time tells them apart.
     */
    private val dropGrace = Runnable { applyScreenOn(false) }

    override fun onCreate(state: Bundle?) {
        super.onCreate(state)

        preferences = getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
        client = AgentClient(preferences, this)

        if (token().isEmpty() || host().isEmpty()) {
            showPairing()
        } else {
            showDeck()
        }

        // Deliberately after setContentView. `window.insetsController` reaches
        // through the decor view, and the decor view is what setContentView
        // installs, so asking for it any earlier throws a NullPointerException
        // rather than failing quietly.
        goFullscreen()
    }

    /**
     * Serve only while there is a screen to serve.
     *
     * Reconnecting here rather than in `onCreate` is the point: a backgrounded
     * panel has nobody looking at it, and a socket nobody is looking at is a
     * heartbeat, a reconnect loop and a radio wake-up every few seconds for
     * nothing. Nothing is lost by closing on the way out — the agent sends a full
     * `Welcome` on every connect, so the next `onStart` re-syncs from scratch.
     */
    override fun onStart() {
        super.onStart()
        visible = true

        if (deckView != null && !terminal) {
            // A panel brought back to the foreground deserves an immediate
            // attempt, not the tail of a backoff earned while it was away.
            backoff.reset()
            applyScreenOn(connected)
            connect()
        }
    }

    override fun onStop() {
        super.onStop()
        visible = false

        // A pending postDelayed outlives being backgrounded, so it has to be
        // cancelled explicitly: otherwise the reconnect fires while the screen is
        // off, which is exactly the wake-up this exists to prevent.
        main.removeCallbacksAndMessages(null)
        reconnectPending = false

        connected = false
        client.close()
        applyScreenOn(false)
    }

    /**
     * Hold the display awake, or let it time out.
     *
     * A control surface should not dim while someone is using it — but this used
     * to be claimed once in `onCreate` and never given back, so the display could
     * never sleep. On a 1600x2560 tablet that is the largest single drain, and it
     * applied even to the pairing form and to a deck whose agent had gone away.
     * It is now held only while a deck is actually live.
     */
    private fun applyScreenOn(on: Boolean) {
        if (on == screenOn) {
            return
        }

        screenOn = on

        if (on) {
            window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        } else {
            window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        }
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        // The bars come back every time the window regains focus, and on a
        // tablet that happens whenever the taskbar is swiped at. A panel that
        // quietly grew a navigation bar is a panel that is no longer full
        // screen, so the claim is re-stated rather than made once.
        if (hasFocus) {
            goFullscreen()
        }
    }

    /**
     * Claim the whole screen: no status bar, no navigation bar, no taskbar.
     *
     * Leaving this to the theme is not enough on a tablet, where the
     * navigation bar is a taskbar that stays put and eats a strip of the deck.
     * The swipe-to-reveal behaviour keeps a way out for anyone who needs the
     * system back gesture.
     */
    private fun goFullscreen() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            window.setDecorFitsSystemWindows(false)
            window.insetsController?.apply {
                hide(WindowInsets.Type.systemBars())
                systemBarsBehavior =
                    WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            }
        } else {
            @Suppress("DEPRECATION")
            window.decorView.systemUiVisibility =
                View.SYSTEM_UI_FLAG_LAYOUT_STABLE or
                View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION or
                View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN or
                View.SYSTEM_UI_FLAG_HIDE_NAVIGATION or
                View.SYSTEM_UI_FLAG_FULLSCREEN or
                View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        client.close()
        main.removeCallbacksAndMessages(null)
    }

    // -- pairing -------------------------------------------------------------

    /**
     * The pairing form.
     *
     * `reason` is set when we arrive here because the agent refused this tablet,
     * so the user is told *why* the deck stopped instead of being returned to a
     * form they thought they had finished with, with no explanation.
     */
    private fun showPairing(reason: String? = null, back: (() -> Unit)? = null) {
        // The deck is gone from the screen, so drop the references: `onStart`
        // uses `deckView` to decide whether there is anything to connect for.
        deckView = null
        strip = null
        panel = null

        val screen = PairingScreen(this, host(), port(), reason, back) { agentHost, agentPort, pin, name, done ->
            client.pair(agentHost, agentPort, pin, name) { result ->
                runOnUiThread {
                    result
                        .onSuccess { token ->
                            prefs().edit()
                                .putString(KEY_HOST, agentHost)
                                .putInt(KEY_PORT, agentPort)
                                .putString(KEY_TOKEN, token)
                                .apply()
                            done(null)
                            // A beat on the green "paired" before the deck takes over.
                            main.postDelayed({
                                showDeck()
                                // `onStart` already ran and will not run again for
                                // this screen, because pairing happened inside it.
                                if (visible) connect()
                            }, 600)
                        }
                        // The agent says *why* — wrong PIN, expired window, too
                        // many attempts — and that is worth showing verbatim.
                        .onFailure { failure ->
                            // A certificate learned from a PC that then said no
                            // vouches for nothing, and kept it would refuse the
                            // next PC tried from this same form.
                            Tls.forget(preferences)
                            done(failure.message ?: "Pairing failed.")
                        }
                }
            }
        }
        setContentView(screen.view())
    }

    // -- the deck ------------------------------------------------------------

    /** Night dimming: checked every minute, and whenever the deck changes. */
    private val screenCare = object : Runnable {
        override fun run() {
            applyScreenCare()
            main.removeCallbacks(this)
            main.postDelayed(this, 60_000L)
        }
    }

    /** Dim the window while the deck's dim window is on; give it back after. */
    private fun applyScreenCare() {
        val care = DeckReader.read(session.config)?.screen ?: return
        val now = java.util.Calendar.getInstance()
        val minute = now.get(java.util.Calendar.HOUR_OF_DAY) * 60 + now.get(java.util.Calendar.MINUTE)
        val wanted = if (care.dimmedAt(minute)) care.dimLevel / 100f else android.view.WindowManager.LayoutParams.BRIGHTNESS_OVERRIDE_NONE
        val attributes = window.attributes
        if (attributes.screenBrightness != wanted) {
            attributes.screenBrightness = wanted
            window.attributes = attributes
        }
    }

    private fun showDeck() {
        // A fresh deck is a fresh attempt, so a previous terminal refusal is no
        // longer in force.
        terminal = false

        val column = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        // A frame around the deck, so the connection panel can lie over it.
        val root = FrameLayout(this).apply { addView(column) }
        deckRoot = root

        val view = DeckView(this)
        view.onPress = { id ->
            client.sendPress(id)
        }
        view.onGesture = { id, gesture ->
            client.sendPress(id, gesture)
        }
        view.onZone = { id, zone ->
            client.sendPress(id, zone = zone)
        }
        view.onSlide = { id, value, drag, lifted ->
            client.sendValue(id, value, drag, lifted)
        }
        view.onPageShown = { pageId ->
            client.sendPage(pageId)
        }
        view.setIcons(bundledIcons)
        view.onNeedImage = { sha ->
            client.fetchImage(sha) { bytes ->
                val bitmap = bytes?.let { android.graphics.BitmapFactory.decodeByteArray(it, 0, it.size) }
                runOnUiThread { view.showImage(sha, bitmap) }
            }
        }
        view.onNeedPack = { id ->
            client.fetchPack(id) { text ->
                val icons = text?.let { dev.flexdeck.client.Icons.readPack(it) }
                runOnUiThread { view.showPack(id, icons) }
            }
        }
        deckView = view

        val status = StatusStrip(this)
        status.show(StatusStrip.Light.WAITING, "Connecting", host())
        status.onDetailTap = { togglePanel() }
        strip = status

        column.addView(status)
        column.addView(
            view,
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f),
        )

        setContentView(root)
        view.show(DeckReader.read(session.config))
        view.showLive(session.liveWidgets)
        view.showSeries(session.series)

        // Connecting is `onStart`'s job, so there is exactly one place that
        // decides when a socket should exist.
    }

    // -- the connection panel ------------------------------------------------

    private var deckRoot: FrameLayout? = null
    private var panel: ConnectionPanel? = null

    private fun togglePanel() {
        panel?.let { it.close(); return }
        val root = deckRoot ?: return
        val open = ConnectionPanel(this, host(), port(), connected, onSwitch = { switchPc() }, onDisconnect = { disconnect() })
        open.onClosed = { if (panel === open) panel = null }
        panel = open
        root.addView(open, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
    }

    /** Off the deck and onto the pairing form: no socket, no retries, no wake lock. */
    private fun leaveDeck() {
        main.removeCallbacksAndMessages(null)
        reconnectPending = false
        connected = false
        client.close()
        applyScreenOn(false)
    }

    /**
     * Pair with another PC. The current pairing is left alone until a new one
     * succeeds, so backing out returns to this deck as it was.
     */
    private fun switchPc() {
        leaveDeck()
        // One certificate is pinned at a time, so another PC's would be refused
        // as "changed". Set this one's aside while pairing, and put it back if
        // the switch is abandoned.
        val pin = Tls.pinnedFingerprint(preferences)
        Tls.forget(preferences)
        showPairing(back = {
            if (pin != null) Tls.restore(preferences, pin)
            showDeck()
            backoff.reset()
            if (visible) connect()
        })
    }

    /**
     * Forget this PC on the tablet: the token and the pinned certificate, as a
     * refusal drops them. The PC is asked to drop this tablet first, while the
     * token and pin still work for that one call. The address stays, so the
     * pairing form can offer it back as the last PC.
     */
    private fun disconnect() {
        leaveDeck()
        client.forgetMe(host(), port(), token()) {
            runOnUiThread {
                Tls.forget(preferences)
                prefs().edit().remove(KEY_TOKEN).apply()
                showPairing()
            }
        }
    }

    private fun connect() {
        reconnectPending = false
        client.connect(host(), port(), token())
    }

    // -- AgentClient.Listener ------------------------------------------------

    /**
     * A frame from the normal conversation.
     *
     * A refusal never arrives here: `AgentClient` reports one through
     * `onRefused`, which stops the loop and offers a re-pair instead of treating
     * a decision as a dropped packet.
     */
    override fun onFrame(message: ServerMessage) {
        session.apply(message)

        runOnUiThread {
            when (message) {
                is ServerMessage.Welcome, is ServerMessage.Patch -> {
                    backoff.reset()
                    deckView?.show(DeckReader.read(session.config))
                    // Where this tablet is, for an agent that knows no page yet.
                    if (message is ServerMessage.Welcome) {
                        deckView?.currentPageId?.let { client.sendPage(it, initial = true) }
                    }
                    screenCare.run()
                    deckView?.setConnected(true)
                    showOnline()
                }

                is ServerMessage.State -> {
                    deckView?.showLive(session.liveWidgets)
                    // Only a failure is worth the status line; a live label
                    // ticking every second is not news.
                    if (!message.ok) {
                        strip?.show(StatusStrip.Light.FAULT, message.error.ifEmpty { "${message.widgetId}: failed" })
                        main.removeCallbacks(clearFault)
                        main.postDelayed(clearFault, FAULT_MILLIS)
                    }
                }

                is ServerMessage.Series -> deckView?.showSeries(session.series)

                is ServerMessage.Page -> deckView?.showPage(message.pageId, message.step)

                is ServerMessage.Failure -> {
                    strip?.show(StatusStrip.Light.FAULT, message.message)
                    main.removeCallbacks(clearFault)
                    main.postDelayed(clearFault, FAULT_MILLIS)
                }
            }

            if (session.needsResync) {
                resync()
            }
        }
    }

    override fun onSocketUp() {
        runOnUiThread {
            connected = true
            backoff.reset()

            // A live deck is worth lighting: cancel a grace period a previous
            // drop may have started.
            main.removeCallbacks(dropGrace)
            applyScreenOn(visible)

            deckView?.setConnected(true)
            showOnline()
        }
    }

    override fun onSocketDown(reason: String) {
        if (terminal) {
            // Already refused, and the pairing form is up. The close that follows
            // a refusal would only overwrite it.
            return
        }

        runOnUiThread {
            // The deck stays on screen, dimmed under a notice rather than
            // cleared: a tablet that shows nothing when the PC sleeps is a
            // tablet that looks broken.
            strip?.show(StatusStrip.Light.WAITING, "Disconnected")
            deckView?.setConnected(false, "Disconnected")

            startedDrop()
        }
    }

    /**
     * A connection ended, and only time will say whether it was a blip.
     *
     * The display is held through a short grace so a momentary drop does not
     * blank a panel someone is watching, then released so an agent that is
     * genuinely gone lets the tablet sleep instead of pinning the screen on for
     * hours. The reconnect is scheduled either way; `visible` and the backoff are
     * what bound how often it actually runs.
     */
    private fun startedDrop() {
        connected = false

        main.removeCallbacks(dropGrace)
        main.postDelayed(dropGrace, DROP_GRACE_MILLIS)

        scheduleReconnect()
    }

    /**
     * The agent will not accept this tablet, and will not change its mind.
     *
     * This is the state that used to be invisible: the refusal arrived, the
     * socket closed a moment later, and the tablet reconnected into the same
     * refusal forever while the status line flickered between the reason and
     * "Reconnecting in Ns". Nothing on the tablet could break the cycle, and the
     * only escape was Android's own "clear storage".
     */
    override fun onRefused(code: String, message: String) {
        terminal = true

        runOnUiThread {
            main.removeCallbacksAndMessages(null)
            reconnectPending = false
            connected = false
            client.close()

            // The pairing form is not a control surface: there is nothing live to
            // look at, so the display is allowed to time out.
            applyScreenOn(false)

            // Both parts of the credential are dropped, not just the token. The
            // pinned certificate is checked *during the TLS handshake*, so a
            // stale pin left behind would make the re-pairing offered below fail
            // before it sent anything — the dead end would simply move.
            Tls.forget(preferences)
            prefs().edit().remove(KEY_TOKEN).apply()

            showPairing(reason = message.ifBlank { code })
        }
    }

    /**
     * Ask for the whole configuration again.
     *
     * The client is holding a document it cannot trust after a patch failed to
     * apply, and the only honest recovery is to throw it away. Applying more
     * patches on top would compound a mistake nothing downstream can see.
     */
    private fun resync() {
        session.reset()
        deckView?.show(null)
        client.close()

        // `close()` is deliberately silent now, so the drop is announced here:
        // without it nothing would restart the grace timer or the reconnect.
        startedDrop()
    }

    private fun scheduleReconnect() {
        // `visible` belongs in the condition rather than being checked later:
        // while the app is off screen there is nobody to reconnect for, and the
        // retry is the wake-up this whole change exists to remove.
        if (reconnectPending || isFinishing || terminal || !visible) {
            return
        }

        reconnectPending = true
        val delay = backoff.nextDelayMillis()
        reconnectAt = SystemClock.uptimeMillis() + delay

        main.removeCallbacks(countdown)
        countdown.run()
        main.postDelayed({ if (!isFinishing) connect() }, delay)
    }

    /** When the next attempt is due, for the countdown on the strip and the notice. */
    private var reconnectAt = 0L

    /** Says how long until the next attempt, once a second, while one is pending. */
    private val countdown: Runnable = object : Runnable {
        override fun run() {
            if (!reconnectPending || connected) return
            val seconds = ((reconnectAt - SystemClock.uptimeMillis()) / 1000).coerceAtLeast(0)
            val words = if (seconds > 0) "Reconnecting in ${seconds}s" else "Reconnecting"
            strip?.show(StatusStrip.Light.WAITING, words)
            deckView?.setConnected(false, words)
            main.postDelayed(this, 1000)
        }
    }

    /** A failure stays on the strip a few seconds, then the strip goes back to saying all is well. */
    private val clearFault = Runnable { if (connected) showOnline() }

    private fun showOnline() {
        val rev = session.rev
        strip?.show(StatusStrip.Light.ONLINE, if (rev > 0) "Online · Rev " + rev.toString().padStart(3, '0') else "Online")
    }

    // -- small helpers -------------------------------------------------------

    private fun prefs() = preferences

    private fun host(): String = preferences.getString(KEY_HOST, "").orEmpty()

    private fun port(): Int = preferences.getInt(KEY_PORT, DEFAULT_PORT)

    private fun token(): String = preferences.getString(KEY_TOKEN, "").orEmpty()

    private companion object {
        const val PREFERENCES = "flexdeck"
        const val KEY_HOST = "host"
        const val KEY_PORT = "port"
        const val KEY_TOKEN = "token"
        const val DEFAULT_PORT = 7878

        /**
         * How long the display stays lit after a drop.
         *
         * Long enough to cover a Wi-Fi hiccup or an agent restart, short enough
         * that a tablet whose PC is asleep is not a nightlight.
         */
        const val DROP_GRACE_MILLIS = 30_000L

        /** How long a failure from the agent stays on the strip. */
        const val FAULT_MILLIS = 6_000L
    }
}

package dev.jraghavan.snmeeting.notes

import android.app.Activity
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.Gravity
import android.view.View
import android.widget.FrameLayout
import android.widget.LinearLayout
import dev.jraghavan.snmeeting.Inkwell
import dev.jraghavan.snmeeting.InkDialog
import dev.jraghavan.snmeeting.NativeBridge
import dev.jraghavan.snmeeting.ScreenScope
import dev.jraghavan.snmeeting.model.CoreJson
import dev.jraghavan.snmeeting.model.CoreResult
import dev.jraghavan.snmeeting.model.str
import dev.jraghavan.snmeeting.record.LiveClock
import org.json.JSONObject

/**
 * DEVICE-UNVERIFIED (wiring of views; the rules are in the pure classes it uses). Layout B's
 * Notes tab (F17-FR1): the infinite canvas with its zoom pill and minimap. Strokes go to the
 * core as they end; the core keeps them in memory and this page flushes them on a short
 * deferred timer ([SAVE_MS], inkread's 1 500 ms) and when it closes, with the viewport.
 *
 * F17-FR12: the page never touches the recording. Leaving it, the screen sleeping or another
 * app in front only saves the ink and gives the pen back; the recording service and the
 * near-live lane carry on. Strokes written once the recording stops are untimed.
 */
class NotesPage(
    private val a: Activity,
    private val ui: Inkwell,
    private val scope: ScreenScope,
    private val id: String,
    private val host: Host,
) {
    interface Host {
        /** The recording under way is this meeting's: its start (elapsed ms), else null. */
        fun recordingSince(): Long?
        /** F17-FR2: the bar's side and state, from settings.json. */
        fun prefs(): NotesPrefs
        fun savePrefs(patch: String)
        /** The timer slot: "0:12:41" while recording, the position when not. */
        fun timer(): String
        /** F16-FR5: "≈ $0.08 so far" while recording near-live, else null. */
        fun cost(): String?
        fun playing(): Boolean
        fun canPlay(): Boolean
        fun togglePlay()
        fun mark()
        fun stop()
        /** F17-FR7: Listen found ink written at a moment: play from [seekMs]; [lineN] is the
         *  transcript line spoken then (or null). */
        fun listenAt(seekMs: Long, lineN: Int?)
        /** The summarise provider ("anthropic"), for the notes notice; null without a key. */
        fun summariseProvider(): String?
    }

    private val handler = Handler(Looper.getMainLooper())
    private val save = Runnable { scope.core({ NativeBridge.nativeInkSave(id) }) { json -> reportError(json, "Couldn't save the notes") } }
    private var opened = false
    /** The Notes tab is in front: only then is the pen claimed for firmware ink. */
    private var front = true

    var tool: NoteTool = NoteTool.PEN
        private set
    /** F17-FR11 / R14: the strip's and the timer's redraws wait while the pen writes. */
    val quiet = PenQuiet()
    /** F17-FR14: dialogs wait while the pen is in use (a dialog takes the pen's claim). */
    val dialogs = DialogGate({ SystemClock.uptimeMillis() })
    private val dialogTick = Runnable { dialogs.tick() }
    /** F21-FR11: the canvas's image copies, decoded for drawing. */
    val images = dev.jraghavan.snmeeting.notes.canvas.ImageCache()

    /** The lasso selection's stroke ids (`[id, …]`), while there is one. */
    private var selected = "[]"

    val canvas: NotesCanvasView = NotesCanvasView(a, object : NotesCanvasView.Host {
        override val tool get() = this@NotesPage.tool
        override val recording get() = host.recordingSince() != null
        override val penWidthPx get() = PEN_WIDTH_PX
        override fun touched() = hideHints()

        override fun pen(down: Boolean) {
            if (down) quiet.down() else quiet.up()
            dialogs.pen(down)
            handler.removeCallbacks(dialogTick)
            if (!down) handler.postDelayed(dialogTick, DialogGate.IDLE_MS)
        }

        override fun strokeTimes(): StrokeTimes {
            val since = host.recordingSince() ?: return StrokeTimes.UNTIMED
            return StrokeTimes.recording(SystemClock.uptimeMillis(), SystemClock.elapsedRealtime(), since) { LiveClock.current }
        }

        override fun commitStroke(tool: NoteTool, widthWorld: Float, xypt: FloatArray) = changed(
            if (tool == NoteTool.PEN) InkBake.Event.PEN_STROKE else InkBake.Event.HIGHLIGHTER_STROKE,
        ) {
            NativeBridge.nativeInkStroke(id, LAYER, tool.core, widthWorld, xypt)
        }

        override fun erase(xy: FloatArray, radiusWorld: Float) = changed(InkBake.Event.ERASE) { NativeBridge.nativeInkErase(id, LAYER, xy, radiusWorld) }
        override fun lasso(xy: FloatArray) = scope.core({ NativeBridge.nativeInkLasso(id, LAYER, xy) }) { json ->
            val o = CoreJson.obj(json) { it }.getOrNull() ?: return@core
            selected = o.optJSONArray("ids")?.toString() ?: "[]"
            canvas.selection = InkView.rect(o.optJSONArray("bbox"))
        }

        override fun moveSelection(dx: Float, dy: Float) {
            val ids = selected
            scope.core({ NativeBridge.nativeInkMove(id, LAYER, ids, dx, dy) }) { json ->
                if (reportError(json, "Couldn't move that")) return@core
                canvas.selection = canvas.selection?.let { WorldRect(it.left + dx, it.top + dy, it.right + dx, it.bottom + dy) }
                fetch(InkBake.Event.LASSO_MOVE)
                scheduleSave()
            }
        }
        override fun listen(x: Float, y: Float, radiusWorld: Float) = scope.core({ NativeBridge.nativeInkHit(id, LAYER, x, y, radiusWorld) }) { json ->
            val hit = CoreJson.obj(json, ListenHit::from).getOrNull() ?: return@core
            val seek = hit.seekMs ?: return@core // an untimed stroke, or no stroke: nothing happens
            hit.lineN?.let { n -> showStrip(strip.state.copy(highlight = n)) }
            host.listenAt(seek, hit.lineN)
        }
        override fun viewportChanged(v: Viewport) = fetch(InkBake.Event.VIEWPORT_SETTLED)
        override fun allEntries(bounds: WorldRect, done: (List<DrawEntry>) -> Unit) = scope.core({
            NativeBridge.nativeCanvasForDraw(id, bounds.left.toFloat(), bounds.top.toFloat(), bounds.right.toFloat(), bounds.bottom.toFloat())
        }) { json -> done(CoreJson.obj(json, InkView::from).getOrNull()?.entries.orEmpty()) }
    })

    /** F17-FR14: the zoom pill's controls the hints point at. */
    private val pillAnchors = HashMap<String, View>()

    private val zoomPill = LinearLayout(a).apply {
        orientation = LinearLayout.HORIZONTAL
        background = ui.outline(ui.dim(30))
        setPadding(ui.dim(8), ui.dim(4), ui.dim(8), ui.dim(4))
        val item = { label: String, act: () -> Unit ->
            ui.eyebrow(label).apply {
                setPadding(ui.dim(12), ui.dim(10), ui.dim(12), ui.dim(10))
                isClickable = true; setOnClickListener { hideHints(); act() }
            }.also { addView(it) }
        }
        pillAnchors["zoom"] = item("FIT") { canvas.fit() }
        Viewport.PRESETS.forEach { p -> item("$p%") { canvas.zoomPercent(p) } }
        pillAnchors["map"] = item("MAP") { canvas.minimapOpen = !canvas.minimapOpen } // F17-FR13: on demand
    }

    /** The canvas with the zoom pill over its top corner. */
    val canvasFrame = FrameLayout(a).apply {
        addView(canvas, FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT))
        addView(zoomPill, FrameLayout.LayoutParams(FrameLayout.LayoutParams.WRAP_CONTENT, FrameLayout.LayoutParams.WRAP_CONTENT,
            Gravity.TOP or Gravity.START).apply { topMargin = ui.dim(12); marginStart = ui.dim(12) })
    }

    /** F17-FR6: the 3-line transcript strip; tapping it opens every line over the lower half. */
    val strip: TranscriptStrip = TranscriptStrip(a, ui) { showStrip(strip.state.toggled()) }

    /** The canvas column: the canvas over the strip. */
    val column = LinearLayout(a).apply {
        orientation = LinearLayout.VERTICAL
        addView(canvasFrame, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, 0, 1f))
        addView(strip.view, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT))
    }

    /** F17-FR3: the meeting's marks (recording-clock ms), each a ◆ beside the stroke before it. */
    private var marks: List<Long> = emptyList()

    fun setMarks(atMs: List<Long>) {
        if (atMs == marks) return
        marks = atMs
        placeMarks()
    }

    private fun placeMarks() {
        val g = MarkGlyphs.place(canvas.inkView.strokes, marks)
        if (g != canvas.markGlyphs) canvas.markGlyphs = g
    }

    /** A meeting with near-live lines has a strip; a Standard one's transcript is on Minutes. */
    var liveAvailable = false
        set(v) { field = v; showStrip(strip.state) }

    /** New lines or a new status from the core (about twice a minute): the strip redraws only if they change it. */
    fun showLive(t: dev.jraghavan.snmeeting.model.LiveTranscript) = quiet.redraw("strip") { showStrip(strip.state.copy(transcript = t)) }

    /** Draw [next]; opened, the strip takes the lower half of the column, its ink above still shown. */
    fun showStrip(next: StripModel) {
        val wasOpen = strip.state.open
        strip.show(next.copy(focus = prefs.focus || !liveAvailable))
        if (next.open != wasOpen) {
            val h = if (next.open) column.height / 2 else LinearLayout.LayoutParams.WRAP_CONTENT
            strip.view.layoutParams = LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, h)
        }
    }

    /** F17-FR2: the docked bar, on the canvas's right (or left) edge, never over it. */
    val bar = DockedToolBar(a, ui, object : DockedToolBar.Host {
        override fun onTool(t: NoteTool) { hideHints(); tool = t; canvas.selection = null; canvas.toolChanged(t); refreshBar() }
        override fun onHelp() = if (hints.showing) hints.hide() else hints.show()
        override fun onUndo() = changed(InkBake.Event.UNDO_REDO) { NativeBridge.nativeInkUndo(id, LAYER) }
        override fun onRedo() = changed(InkBake.Event.UNDO_REDO) { NativeBridge.nativeInkRedo(id, LAYER) }
        override fun onCollapse(collapsed: Boolean) {
            host.savePrefs(NotesPatch.collapsed(collapsed))
            prefs = prefs.copy(collapsed = collapsed)
            layout()
        }
        override fun onOptions(anchor: View) = options()
        override fun onMark() = host.mark()
        override fun onStop() = host.stop()
        override fun onPlay() { host.togglePlay(); refreshBar() }
    })

    private var prefs = host.prefs()

    /** The tab's content: the bar beside the canvas column. */
    private val row = LinearLayout(a).apply { orientation = LinearLayout.HORIZONTAL }

    /** F17-FR14: the hand-drawn hints, over everything on the tab; they never take a touch. */
    private val hints: HintsOverlay = HintsOverlay(a, ::hintTips, ::inkCovers) { prefs.dock == NotesPrefs.Dock.LEFT }

    /** The tab's content: the bar beside the canvas column, the hints over both. */
    val view = FrameLayout(a).apply {
        addView(row, FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT))
        addView(hints, FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT))
    }

    fun hideHints() = hints.hide()

    /** Each control's arrow tip in the hints' pixels: the side of a bar control facing the
     *  canvas, the bottom of a zoom-pill control, the top of the strip. */
    private fun hintTips(): Map<String, Pair<Float, Float>> {
        val origin = IntArray(2).also { hints.getLocationInWindow(it) }
        fun box(v: View): FloatArray {
            val p = IntArray(2).also { v.getLocationInWindow(it) }
            return floatArrayOf((p[0] - origin[0]).toFloat(), (p[1] - origin[1]).toFloat(), v.width.toFloat(), v.height.toFloat())
        }
        val out = HashMap<String, Pair<Float, Float>>()
        val left = prefs.dock == NotesPrefs.Dock.LEFT
        bar.anchors.forEach { (k, v) -> val b = box(v); out[k] = (if (left) b[0] + b[2] else b[0]) to b[1] + b[3] / 2 }
        pillAnchors.forEach { (k, v) -> val b = box(v); out[k] = b[0] + b[2] / 2 to b[1] + b[3] }
        if (strip.view.visibility == View.VISIBLE) { val b = box(strip.view); out["strip"] = b[0] + b[2] / 2 to b[1] }
        return out
    }

    /** Whether a box in the hints' pixels covers any ink on the canvas. */
    private fun inkCovers(l: Float, t: Float, r: Float, b: Float): Boolean {
        val o = IntArray(2).also { hints.getLocationInWindow(it) }
        val c = IntArray(2).also { canvas.getLocationInWindow(it) }
        val v = canvas.viewport
        val boxes = canvas.inkView.strokes.map { it.bounds } + canvas.inkView.items.map { it.bounds }
        return boxes.any { w ->
            val sl = v.screenX(w.left) + c[0] - o[0]; val sr = v.screenX(w.right) + c[0] - o[0]
            val st = v.screenY(w.top) + c[1] - o[1]; val sb = v.screenY(w.bottom) + c[1] - o[1]
            sl < r && l < sr && st < b && t < sb
        }
    }

    init {
        canvas.images = images
        canvas.inkHoles = { listOf(zoomPill) } // F17-FR4: the pen never inks over the pill
        // F17-FR13: the minimap card sits under the zoom pill when the bar leaves it that corner.
        canvas.minimapTop = { if (canvas.barOnLeft) 0 else zoomPill.bottom }
        layout()
    }

    /** Bar and column in dock order, the bar at its expanded or collapsed width: one redraw. */
    private fun layout() {
        row.removeAllViews()
        val barParams = LinearLayout.LayoutParams(if (prefs.collapsed) bar.collapsedPx else bar.expandedPx,
            LinearLayout.LayoutParams.MATCH_PARENT)
        val colParams = LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.MATCH_PARENT, 1f)
        val rule = View(a).apply { setBackgroundColor(ui.ink) }
        val ruleParams = LinearLayout.LayoutParams(maxOf(1, ui.dp(1)), LinearLayout.LayoutParams.MATCH_PARENT)
        if (prefs.dock == NotesPrefs.Dock.LEFT) {
            row.addView(bar.view, barParams); row.addView(rule, ruleParams); row.addView(column, colParams)
        } else {
            row.addView(column, colParams); row.addView(rule, ruleParams); row.addView(bar.view, barParams)
        }
        canvas.barOnLeft = prefs.dock == NotesPrefs.Dock.LEFT
        refreshBar()
    }

    /** The bar's state now; it rebuilds only when its items change, else edits in place. */
    fun refreshBar() {
        val recording = host.recordingSince() != null
        if (recording && tool == NoteTool.LISTEN) tool = NoteTool.PEN // Listen is off while recording
        val ink = canvas.inkView
        bar.render(ToolBarModel.State(tool, prefs.collapsed, recording, ink.canUndo, ink.canRedo,
            host.timer(), host.cost(), host.playing(), host.canPlay(), prefs.focus))
    }

    /** The tool options: Focus, Fit, a full refresh of the panel (F17-FR11, the user's choice). */
    private fun options() {
        val items = arrayOf(if (prefs.focus) "Focus off" else "Focus on", "Fit all ink", "Refresh the screen")
        android.app.AlertDialog.Builder(a, dev.jraghavan.snmeeting.R.style.InkDialog).setTitle("Notes")
            .setItems(items) { _, i ->
                when (i) {
                    0 -> {
                        prefs = prefs.copy(focus = !prefs.focus)
                        host.savePrefs(NotesPatch.focus(prefs.focus))
                        showStrip(strip.state) // F17-AC8: hidden, or back with the newest lines
                        refreshBar()
                    }
                    1 -> canvas.fit()
                    else -> canvas.fullRefresh()
                }
            }
            .setNegativeButton("CLOSE", null).show()
    }

    /** Opens the canvas at its last viewport, or fitted to its ink; or (F19-FR3) centred on
     *  [focus], a search hit's line, outlined with a 2 px dashed box. */
    fun open(focus: WorldRect? = null) {
        scope.core({ NativeBridge.nativeNotesOpen(id) }) { json ->
            when (val r = CoreJson.obj(json) { it }) {
                is CoreResult.Ok -> {
                    opened = true
                    val o = r.value
                    o.str("imagesDir")?.let { images.folder = java.io.File(it) }
                    canvas.post {
                        val w = canvas.width.toDouble(); val h = canvas.height.toDouble()
                        val v = o.optJSONObject("view")
                        canvas.outline = focus
                        val next = if (focus != null) {
                            Viewport.centred((focus.left + focus.right) / 2, (focus.top + focus.bottom) / 2,
                                v?.optDouble("zoom", 1.0) ?: 1.0, w, h)
                        } else if (v != null) {
                            Viewport.centred(v.optDouble("cx"), v.optDouble("cy"), v.optDouble("zoom", 1.0), w, h)
                        } else {
                            Viewport.fitInk(InkView.rect(o.optJSONArray("bounds")), w, h, NotesCanvasView.FIT_PAD)
                        }
                        canvas.setViewport(next)
                        if (front) canvas.claimPen("open")
                        // F17-FR14: an empty canvas opens with the hints, one with ink without them.
                        if (o.optInt("strokes", 0) == 0 && o.optInt("items", 0) == 0) hints.show()
                    }
                }
                is CoreResult.Err -> dialogs.show { InkDialog.error(a, "The notes can't be opened", r.message) }
            }
        }
    }

    /** inkread's applyToolInkState: re-claim the pen on every resume and focus gain (the
     *  firmware drops the claim when another window takes focus), while this tab is in front. */
    fun claim(reason: String) { if (opened && front) canvas.claimPen(reason) }

    /** inkread's onPause: the claim is given back (else the pen inks other apps, #157). */
    fun pause() = canvas.releasePen()

    /** The tab came to the front or went behind the Minutes tab. */
    fun shown(visible: Boolean) {
        if (visible == front) return
        front = visible
        if (!opened) return
        if (visible) canvas.claimPen("tab") else { canvas.bake(InkBake.Event.PAGE_LEAVING); canvas.releasePen() }
    }

    /** The page goes: the pen is let go, the ink saved and the viewport kept. */
    fun close() {
        handler.removeCallbacks(save)
        canvas.releasePen()
        if (!opened) return
        opened = false
        val (cx, cy) = canvas.viewport.centre(canvas.width.toDouble(), canvas.height.toDouble())
        val z = canvas.viewport.zoom
        scope.core({ NativeBridge.nativeNotesClose(id, cx.toFloat(), cy.toFloat(), z.toFloat()) }) { json ->
            reportError(json, "Couldn't save the notes")
        }
    }

    /** A change to the ink: run [call], fetch what is in view (drawn once the pen rests) and
     *  arm the deferred save. */
    private fun changed(event: InkBake.Event, call: () -> String) {
        scope.core(call) { json ->
            if (!reportError(json, "Couldn't keep that")) {
                fetch(event)
                scheduleSave()
            }
        }
    }

    private fun scheduleSave() {
        handler.removeCallbacks(save)
        handler.postDelayed(save, SAVE_MS)
    }

    /** The strokes in view (plus a margin, so a small pan needs no fetch). */
    fun fetch(event: InkBake.Event?) {
        val v = canvas.viewport
        val r = v.visible(canvas.width.toDouble(), canvas.height.toDouble())
        val m = maxOf(r.width, r.height) / 2
        scope.core({
            NativeBridge.nativeCanvasForDraw(id, (r.left - m).toFloat(), (r.top - m).toFloat(),
                (r.right + m).toFloat(), (r.bottom + m).toFloat())
        }) { json ->
            CoreJson.obj(json, InkView::from).getOrNull()?.let {
                val history = it.canUndo != canvas.inkView.canUndo || it.canRedo != canvas.inkView.canRedo
                canvas.show(it, event)
                placeMarks()
                if (history) refreshBar() // Undo and Redo light up or grey out
            }
        }
    }

    /** True when [json] is an error, which is then shown. */
    private fun reportError(json: String, title: String): Boolean {
        val e = CoreJson.obj(json) { o: JSONObject -> o } as? CoreResult.Err ?: return false
        dialogs.show { InkDialog.error(a, title, e.message) }
        return true
    }

    companion object {
        const val LAYER = "canvas"
        const val SAVE_MS = 1_500L
        /** inkread's default nib (`StylusInkController.PEN_WIDTHS[2]`): matches the firmware's. */
        const val PEN_WIDTH_PX = 9f
    }
}

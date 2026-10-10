package dev.jraghavan.snmeeting.notes

import android.annotation.SuppressLint
import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.DashPathEffect
import android.graphics.Paint
import android.graphics.Path
import android.os.SystemClock
import android.view.MotionEvent
import android.view.ScaleGestureDetector
import android.view.View
import android.view.ViewTreeObserver
import kotlin.math.hypot

/**
 * DEVICE-UNVERIFIED (pen, palm and e-ink behaviour need the Manta). The Notes tab's infinite
 * canvas (F13-FR1, F17-FR1/FR4/FR7): a thin adapter that maps MotionEvents through
 * [TouchArbiter], [PalmRule] and [InputRules] and draws the strokes the core holds. Everything
 * it decides is in those pure classes and in [Viewport]; this file only reads events and paints.
 *
 * - The pen writes with the selected tool; firmware ink ([SupernoteInk]) paints the live
 *   stroke, so pen-up commits without a redraw; the canvas redraws once the pen has rested.
 * - A finger drag pans, a pinch zooms (5 %–2000 %), a finger tap does nothing unless Listen
 *   is selected. A palm (pen near, or a large contact) does nothing.
 * - Nothing else redraws the canvas but ink, pan/zoom and the playback emphasis (F17-FR11).
 */
@SuppressLint("ViewConstructor")
class NotesCanvasView(
    context: Context,
    private val host: Host,
    /** F17-FR10's ink area on the Minutes tab: a fixed sheet in a scrolling page. A finger
     *  scrolls the page (it is not taken here) and the canvas draws the live stroke itself. */
    private val fixed: Boolean = false,
) : View(context) {

    interface Host {
        /** The pen landed or lifted (F17-FR11: other redraws wait for the lift). */
        fun pen(down: Boolean) {}
        /** Any touch on the canvas, pen or finger (F17-FR14: it dismisses the hints). */
        fun touched() {}
        val tool: NoteTool
        val recording: Boolean
        /** The times for a stroke starting now (F17-FR4). */
        fun strokeTimes(): StrokeTimes
        /** Pen stroke ended: `[x, y, pressure, t]` world points, width in world units. */
        fun commitStroke(tool: NoteTool, widthWorld: Float, xypt: FloatArray)
        fun erase(xy: FloatArray, radiusWorld: Float)
        fun lasso(xy: FloatArray)
        fun moveSelection(dx: Float, dy: Float)
        /** A Listen tap at a world point (F17-FR7). */
        fun listen(x: Float, y: Float, radiusWorld: Float)
        /** The viewport moved: the visible strokes are fetched again. */
        fun viewportChanged(v: Viewport)
        val penWidthPx: Float
    }

    var viewport = Viewport(0.0, 0.0, 1.0)
        private set
    private var ink: InkView = InkView.EMPTY
    /** Playback position for the emphasis (F17-FR7), or null when not playing. */
    var positionMs: Long? = null
        private set
    /** A search hit's line, outlined with a 2 px dashed box (F19-FR3). */
    var outline: WorldRect? = null
        set(v) { field = v; invalidate() }
    /** The lasso selection's box, while there is one. */
    var selection: WorldRect? = null
        set(v) { field = v; invalidate() }
    /** F17-FR3: ◆ beside the stroke written before each mark. */
    var markGlyphs: List<Pair<Double, Double>> = emptyList()
        set(v) { field = v; invalidate() }
    var barOnLeft = false

    private val arbiter = TouchArbiter()
    private var buffer: StrokeBuffer? = null
    private var gesture = Gesture.NONE
    private var penTool = NoteTool.PEN
    private val path = ArrayList<Float>()
    private var downX = 0f
    private var downY = 0f
    private var lastX = 0f
    private var lastY = 0f
    private var moved = false
    private var movingSelection = false
    private var penHovering = false
    private var lastPenAt = -1_000_000L
    private var minimapUntil = 0L
    /** F17-FR13: opened from the pill; stays until closed or the pen touches the canvas. */
    var minimapOpen = false
        set(v) { field = v; invalidate(); updateInkAreas() }
    /** A finger on the minimap: the frame held for the gesture, where it began, and whether it
     *  grabbed the view rectangle (a drag) or not (a jump to the finger). */
    private var mapFrame: MinimapFrame? = null
    private var mapStart: Viewport? = null
    private var mapGrab = false
    private var mapDown = 0.0 to 0.0

    private val firmware = SupernoteInk(context)
    private var firmwareOn = false

    /** F17-FR4: the controls drawn over the canvas (the zoom pill) where the pen must not ink. */
    var inkHoles: () -> List<View> = { emptyList() }

    /** Any layout on the page (bar collapse, dock side, the strip opening, Focus) may move the
     *  canvas: the disabled areas follow, sent only when they change. */
    private val layoutWatch = ViewTreeObserver.OnGlobalLayoutListener { updateInkAreas() }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        if (!fixed) viewTreeObserver.addOnGlobalLayoutListener(layoutWatch)
    }

    private fun screenRect(v: View): PxRect {
        val p = IntArray(2).also { v.getLocationOnScreen(it) }
        return PxRect(p[0], p[1], p[0] + v.width, p[1] + v.height)
    }

    /**
     * F17-FR4: tell the firmware pen where not to ink: everything outside this canvas on the
     * screen, the zoom pill and the open minimap. Screen pixels (getLocationOnScreen); the
     * window offset is logged beside them so the device log shows whether the two differ.
     */
    fun updateInkAreas() {
        if (fixed || !isAttachedToWindow || width == 0 || height == 0) return
        val c = screenRect(this)
        val root = rootView
        val r = screenRect(root)
        val dm = resources.displayMetrics
        val screen = PxRect(0, 0, maxOf(r.right, dm.widthPixels), maxOf(r.bottom, dm.heightPixels))
        val holes = inkHoles().filter { it.isShown && it.width > 0 }.map(::screenRect).toMutableList()
        if (minimapOpen) mapFrameNow()?.let { f ->
            holes += PxRect(c.left + f.left.toInt(), c.top + f.top.toInt(),
                c.left + (f.left + f.width).toInt() + 1, c.top + (f.top + f.height).toInt() + 1)
        }
        val w = IntArray(2).also { getLocationInWindow(it) }
        firmware.setDisabledAreas(InkAreas.disabled(screen, c, holes),
            "(screen px; canvas window=${w[0]},${w[1]} screen=${c.left},${c.top} size=${width}x$height)")
    }

    private val inkPaint = Paint().apply {
        style = Paint.Style.STROKE; strokeCap = Paint.Cap.ROUND; strokeJoin = Paint.Join.ROUND; isAntiAlias = true
    }
    private val dotPaint = Paint().apply { style = Paint.Style.FILL; isAntiAlias = true }
    private val livePaint = Paint().apply {
        style = Paint.Style.STROKE; strokeCap = Paint.Cap.ROUND; strokeJoin = Paint.Join.ROUND; isAntiAlias = true
        color = Color.argb(96, 0, 0, 0)
    }
    private val dashPaint = Paint().apply {
        style = Paint.Style.STROKE; color = Color.BLACK; strokeWidth = 2f
        pathEffect = DashPathEffect(floatArrayOf(10f, 8f), 0f)
    }
    private val framePaint = Paint().apply { style = Paint.Style.STROKE; color = Color.BLACK; strokeWidth = 2f }
    private val glyphPaint = Paint().apply { color = Color.BLACK; textSize = 28f; isAntiAlias = true }

    private val drawPath = Path()
    private val livePath = Path()
    /** InkBake's steps still to run once the canvas's redraw is on the panel. */
    private var afterDraw: List<InkBake.Step> = emptyList()
    private val hideMinimap = Runnable { invalidate() }

    private val scale = ScaleGestureDetector(context, object : ScaleGestureDetector.SimpleOnScaleGestureListener() {
        override fun onScale(d: ScaleGestureDetector): Boolean {
            if (arbiter.isPenActive || palmNear()) return true
            setViewport(viewport.zoomAbout(d.scaleFactor.toDouble(), d.focusX.toDouble(), d.focusY.toDouble()), notify = false)
            return true
        }

        override fun onScaleEnd(d: ScaleGestureDetector) = host.viewportChanged(viewport)
    })

    /** The strokes in view and the history state, from the core. [event] says whether the
     *  canvas bakes now (InkBake): a pen stroke never does, its ink stays on the overlay. */
    fun show(next: InkView, event: InkBake.Event? = InkBake.Event.VIEWPORT_SETTLED) {
        ink = next
        event?.let(::bake)
    }

    /** F17-FR4: run InkBake's plan for [e]: draw, then (after that frame) refresh the panel,
     *  then clear the overlay. Never clear first. */
    fun bake(e: InkBake.Event) {
        val steps = InkBake.plan(e, InkBake.overlayFor(host.tool))
        if (steps.isEmpty()) return
        if (InkBake.Step.DRAW in steps) {
            afterDraw = steps - InkBake.Step.DRAW
            invalidate()
        } else {
            steps.forEach(::step)
        }
    }

    private fun step(s: InkBake.Step) {
        when (s) {
            InkBake.Step.DRAW -> invalidate()
            InkBake.Step.REFRESH_PANEL -> if (firmwareOn) firmware.fullRefresh()
            InkBake.Step.CLEAR_OVERLAY -> if (firmwareOn) firmware.clearAll()
        }
    }

    val inkView: InkView get() = ink

    /** The canvas moves under the finger; the overlay is cleared only once the move settles and
     *  the canvas's redraw is on the panel (the host's fetch bakes with VIEWPORT_SETTLED). */
    fun setViewport(v: Viewport, notify: Boolean = true) {
        viewport = v
        showMinimap()
        invalidate()
        if (notify) host.viewportChanged(v)
    }

    /** The playback emphasis moved: redraw only when it changes which strokes are muted. */
    fun setPosition(ms: Long?) {
        val before = positionMs
        positionMs = ms
        if (Emphasis.changed(ink.strokes, before, ms)) invalidate()
    }

    fun fit() = setViewport(Viewport.fitInk(ink.bounds, width.toDouble(), height.toDouble(), FIT_PAD))

    fun zoomPercent(p: Int) = setViewport(viewport.zoomToPercent(p, width.toDouble(), height.toDouble()))

    /** Firmware ink on for the Pen only (inkread's applyToolInkState); the other tools draw
     *  their own band or loop, and a change to one bakes the page. */
    fun toolChanged(t: NoteTool) {
        if (!firmwareOn) firmwareOn = firmware.setup()
        firmware.setWritable(InkBake.overlayFor(t))
        bake(InkBake.Event.TOOL_CHANGED)
    }

    /** inkread's applyToolInkState: claim the pen on every resume and focus gain (the firmware
     *  drops the claim when another window takes focus), writable for the Pen. Clears nothing. */
    fun claimPen(reason: String) {
        updateInkAreas()
        firmwareOn = firmware.setup(reason)
        firmware.systemGestures(false)
        firmware.setWritable(InkBake.overlayFor(host.tool))
    }

    fun releasePen() { firmware.teardown(); firmware.systemGestures(true); firmwareOn = false }

    fun fullRefresh() = bake(InkBake.Event.USER_REFRESH)

    private fun showMinimap() {
        minimapUntil = SystemClock.uptimeMillis() + MinimapFrame.LINGER_MS
        removeCallbacks(hideMinimap)
        postDelayed(hideMinimap, MinimapFrame.LINGER_MS)
    }

    private fun minimapShown() = minimapOpen || SystemClock.uptimeMillis() < minimapUntil

    private fun mapFrameNow() = MinimapFrame.of(ink.bounds, viewport, width.toDouble(), height.toDouble(), barOnLeft)

    /**
     * F17-FR13 (sn-canvas): a finger on the shown minimap navigates. A tap moves the view to
     * the point; a drag of the view rectangle pans by the world distance, a drag elsewhere
     * follows the finger. Nothing redraws until the finger lifts; then one redraw, baked in
     * F17-FR4's order (draw, refresh, clear). Returns false when the minimap isn't touched.
     */
    private fun minimapTouch(e: MotionEvent): Boolean {
        val x = e.x.toDouble(); val y = e.y.toDouble()
        val w = width.toDouble(); val h = height.toDouble()
        when (e.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                val f = mapFrameNow()?.takeIf { minimapShown() && it.contains(x, y) } ?: return false
                mapFrame = f; mapStart = viewport; mapDown = x to y
                mapGrab = f.onView(x, y, viewport, w, h)
                return true
            }
            MotionEvent.ACTION_MOVE -> return mapFrame != null
            MotionEvent.ACTION_UP -> {
                val f = mapFrame ?: return false
                val start = mapStart ?: viewport
                val target = if (mapGrab) f.dragBy(start, x - mapDown.first, y - mapDown.second) else f.centreOn(x, y, start, w, h)
                mapFrame = null; mapStart = null
                setViewport(target) // the host fetches what is now in view and bakes once
                return true
            }
            MotionEvent.ACTION_CANCEL -> { val had = mapFrame != null; mapFrame = null; mapStart = null; return had }
        }
        return mapFrame != null
    }

    private fun palmNear(): Boolean = PalmRule.isPalm(penHovering, arbiter.isPenActive,
        SystemClock.uptimeMillis() - lastPenAt, 0f, height)

    private fun contact(e: MotionEvent, i: Int) = Contact(
        id = e.getPointerId(i),
        isPen = e.getToolType(i).let { it == MotionEvent.TOOL_TYPE_STYLUS || it == MotionEvent.TOOL_TYPE_ERASER },
        x = e.getX(i), y = e.getY(i), pressure = e.getPressure(i),
        isEraser = e.getToolType(i) == MotionEvent.TOOL_TYPE_ERASER,
    )

    override fun onHoverEvent(e: MotionEvent): Boolean {
        if (e.getToolType(0) == MotionEvent.TOOL_TYPE_STYLUS || e.getToolType(0) == MotionEvent.TOOL_TYPE_ERASER) {
            penHovering = e.actionMasked != MotionEvent.ACTION_HOVER_EXIT
            lastPenAt = SystemClock.uptimeMillis()
        }
        return super.onHoverEvent(e)
    }

    @SuppressLint("ClickableViewAccessibility") // the canvas is not a button: taps are Listen's
    override fun onTouchEvent(e: MotionEvent): Boolean {
        val pen = e.getToolType(e.actionIndex).let { it == MotionEvent.TOOL_TYPE_STYLUS || it == MotionEvent.TOOL_TYPE_ERASER }
        if (pen && e.actionMasked == MotionEvent.ACTION_DOWN && minimapOpen) minimapOpen = false // the pen closes it
        if (!fixed && !pen && e.pointerCount == 1 && minimapTouch(e)) return true
        if (fixed && e.actionMasked == MotionEvent.ACTION_DOWN && !pen) return false // the page scrolls
        if (e.actionMasked == MotionEvent.ACTION_DOWN) host.touched()
        if (fixed) parent?.requestDisallowInterceptTouchEvent(true)
        if (!arbiter.isPenActive && !fixed) scale.onTouchEvent(e)
        val inputs: List<PointerInput> = when (e.actionMasked) {
            MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> {
                val c = contact(e, e.actionIndex)
                if (!c.isPen && PalmRule.isPalm(penHovering, arbiter.isPenActive, SystemClock.uptimeMillis() - lastPenAt,
                        e.getTouchMajor(e.actionIndex), height)) emptyList() else arbiter.down(c)
            }
            MotionEvent.ACTION_MOVE -> {
                if (arbiter.followedId != null) historical(e)
                listOfNotNull(arbiter.move((0 until e.pointerCount).map { contact(e, it) }))
            }
            MotionEvent.ACTION_POINTER_UP -> listOfNotNull(arbiter.up(contact(e, e.actionIndex), isLastContact = false))
            MotionEvent.ACTION_UP -> listOfNotNull(arbiter.up(contact(e, e.actionIndex), isLastContact = true))
            MotionEvent.ACTION_CANCEL -> listOfNotNull(arbiter.cancel())
            else -> emptyList()
        }
        inputs.forEach { handle(it, e) }
        return true
    }

    /** The pen's batched samples, with their own times (F17-FR4). */
    private fun historical(e: MotionEvent) {
        val b = buffer ?: return
        val i = e.findPointerIndex(arbiter.followedId ?: return).takeIf { it >= 0 } ?: return
        for (h in 0 until e.historySize) {
            b.add(e.getHistoricalX(i, h), e.getHistoricalY(i, h), e.getHistoricalPressure(i, h), e.getHistoricalEventTime(h))
        }
    }

    private fun handle(input: PointerInput, e: MotionEvent) {
        when (input) {
            is PointerInput.Start -> start(input.contact, e.eventTime)
            is PointerInput.Move -> move(input, e.eventTime)
            is PointerInput.End -> end(input.x, input.y, e.eventTime)
            PointerInput.Abandon -> abandon()
        }
    }

    private fun start(c: Contact, at: Long) {
        downX = c.x; downY = c.y; lastX = c.x; lastY = c.y; moved = false
        path.clear()
        if (c.isPen) {
            host.pen(true)
            lastPenAt = SystemClock.uptimeMillis()
            penTool = NoteTool.forPen(host.tool, c.isEraser)
            gesture = InputRules.forPen(host.tool, c.isEraser, host.recording)
            when (gesture) {
                Gesture.DRAW -> buffer = StrokeBuffer(host.strokeTimes()).also { it.add(c.x, c.y, c.pressure, at) }
                Gesture.ERASE, Gesture.LASSO -> {
                    if (c.isEraser && firmwareOn) firmware.setWritable(false)
                    val sel = selection
                    movingSelection = gesture == Gesture.LASSO && sel != null &&
                        sel.contains(viewport.worldX(c.x.toDouble()), viewport.worldY(c.y.toDouble()))
                    path += c.x; path += c.y
                }
                else -> Unit
            }
        } else {
            gesture = Gesture.PAN // a tap is decided at the end
        }
    }

    private fun move(m: PointerInput.Move, at: Long) {
        if (hypot(m.x - downX, m.y - downY) > TAP_SLOP_PX) moved = true
        when (gesture) {
            Gesture.DRAW -> {
                buffer?.add(m.x, m.y, m.pressure, at)
                if (!overlayInks()) invalidate() // no firmware ink here: the canvas draws the live stroke
            }
            Gesture.ERASE, Gesture.LASSO -> {
                path += m.x; path += m.y
                if (!movingSelection) invalidate() // the band or loop is drawn by the canvas itself
            }
            Gesture.PAN -> if (moved && !palmNear()) {
                setViewport(viewport.panBy((m.x - lastX).toDouble(), (m.y - lastY).toDouble()), notify = false)
            }
            else -> Unit
        }
        lastX = m.x; lastY = m.y
    }

    private fun end(x: Float, y: Float, at: Long) {
        val was = gesture
        val wx = { vx: Float -> viewport.worldX(vx.toDouble()).toFloat() }
        val wy = { vy: Float -> viewport.worldY(vy.toDouble()).toFloat() }
        when (gesture) {
            Gesture.DRAW -> buffer?.let { b ->
                b.add(x, y, 1f, at)
                val width = (host.penWidthPx / viewport.zoom).toFloat()
                host.commitStroke(penTool, if (penTool == NoteTool.HIGHLIGHTER) width * HIGHLIGHT_FACTOR else width,
                    b.pack { vx, vy -> wx(vx) to wy(vy) })
                // A pen stroke stays on the firmware overlay: nothing is redrawn or cleared now
                // (inkread). The highlighter's stroke bakes when the core has it.
            }
            Gesture.ERASE -> {
                path += x; path += y
                host.erase(world(path), (ERASE_RADIUS_PX / viewport.zoom).toFloat())
                if (firmwareOn) firmware.setWritable(InkBake.overlayFor(host.tool))
            }
            Gesture.LASSO -> if (movingSelection) {
                host.moveSelection(wx(x) - wx(downX), wy(y) - wy(downY))
            } else if (moved) {
                path += x; path += y
                host.lasso(world(path))
            } else {
                selection = null
            }
            Gesture.LISTEN -> host.listen(wx(x), wy(y), (LISTEN_RADIUS_PX / viewport.zoom).toFloat())
            Gesture.PAN -> if (moved) {
                host.viewportChanged(viewport) // the strokes now in view, once the drag ends
            } else {
                if (InputRules.forFinger(host.tool, isTap = true, palm = palmNear(), recording = host.recording) == Gesture.LISTEN) {
                    host.listen(wx(x), wy(y), (LISTEN_RADIUS_PX / viewport.zoom).toFloat())
                }
            }
            Gesture.NONE -> Unit
        }
        buffer = null; path.clear(); gesture = Gesture.NONE; movingSelection = false
        host.pen(false)
        // A pen stroke the firmware inked is not repainted (inkread); anything else drew a band
        // or loop of its own, which the next draw removes.
        if (was != Gesture.DRAW || !overlayInks(was)) invalidate()
    }

    /** Whether the stroke being written is the firmware's (the Pen, with the overlay claimed). */
    private fun overlayInks(g: Gesture = gesture): Boolean = firmwareOn && g == Gesture.DRAW && penTool == NoteTool.PEN

    private fun abandon() {
        buffer = null; path.clear(); gesture = Gesture.NONE; movingSelection = false
        host.pen(false)
        invalidate()
    }

    private fun world(xy: List<Float>): FloatArray = FloatArray(xy.size) { i ->
        if (i % 2 == 0) viewport.worldX(xy[i].toDouble()).toFloat() else viewport.worldY(xy[i].toDouble()).toFloat()
    }

    override fun onDraw(canvas: Canvas) {
        canvas.drawColor(Color.WHITE)
        val v = viewport
        val z = v.zoom.toFloat()
        for (s in ink.strokes) {
            val muted = Emphasis.muted(s, positionMs)
            val color = when {
                muted -> MUTED
                s.highlighter -> Color.argb(96, 0, 0, 0)
                else -> s.argb or 0xFF000000.toInt()
            }
            inkPaint.color = color; dotPaint.color = color
            inkPaint.strokeWidth = (s.width * z).coerceAtLeast(1f)
            if (s.pts.size == 2) {
                canvas.drawCircle(v.screenX(s.pts[0].toDouble()).toFloat(), v.screenY(s.pts[1].toDouble()).toFloat(),
                    inkPaint.strokeWidth / 2, dotPaint)
                continue
            }
            drawPath.reset()
            drawPath.moveTo(v.screenX(s.pts[0].toDouble()).toFloat(), v.screenY(s.pts[1].toDouble()).toFloat())
            var i = 2
            while (i + 1 < s.pts.size) {
                drawPath.lineTo(v.screenX(s.pts[i].toDouble()).toFloat(), v.screenY(s.pts[i + 1].toDouble()).toFloat()); i += 2
            }
            canvas.drawPath(drawPath, inkPaint)
        }
        val live = buffer
        if (gesture == Gesture.DRAW && !overlayInks() && live != null && live.size > 1) {
            val xy = live.xy()
            livePath.reset()
            livePath.moveTo(xy[0], xy[1])
            var k = 2
            while (k + 1 < xy.size) { livePath.lineTo(xy[k], xy[k + 1]); k += 2 }
            inkPaint.color = Color.BLACK; inkPaint.strokeWidth = host.penWidthPx
            canvas.drawPath(livePath, inkPaint)
        }
        markGlyphs.forEach { (x, y) -> canvas.drawText("◆", v.screenX(x).toFloat(), v.screenY(y).toFloat(), glyphPaint) }
        outline?.let { box(canvas, it, dashPaint) }
        selection?.let { box(canvas, it, dashPaint) }
        if ((gesture == Gesture.ERASE || gesture == Gesture.LASSO) && !movingSelection && path.size >= 4) {
            livePaint.strokeWidth = if (gesture == Gesture.ERASE) ERASE_RADIUS_PX * 2 else 2f
            livePath.reset()
            livePath.moveTo(path[0], path[1])
            var i = 2
            while (i + 1 < path.size) { livePath.lineTo(path[i], path[i + 1]); i += 2 }
            canvas.drawPath(livePath, if (gesture == Gesture.LASSO) dashPaint else livePaint)
        }
        if (minimapShown()) minimap(canvas)
        // F17-FR4: the panel shows this frame; only now refresh and clear the overlay.
        if (afterDraw.isNotEmpty()) {
            val rest = afterDraw
            afterDraw = emptyList()
            post { rest.forEach(::step) }
        }
    }

    private fun box(canvas: Canvas, r: WorldRect, paint: Paint) {
        val v = viewport
        canvas.drawRect(v.screenX(r.left).toFloat() - 4, v.screenY(r.top).toFloat() - 4,
            v.screenX(r.right).toFloat() + 4, v.screenY(r.bottom).toFloat() + 4, paint)
    }

    /** sn-canvas's minimap: all ink and the part in view, one repaint (F17-FR1). */
    private fun minimap(canvas: Canvas) {
        val f = MinimapFrame.of(ink.bounds, viewport, width.toDouble(), height.toDouble(), barOnLeft) ?: return
        val l = f.left.toFloat(); val t = f.top.toFloat()
        dotPaint.color = Color.WHITE
        canvas.drawRect(l, t, l + f.width.toFloat(), t + f.height.toFloat(), dotPaint)
        canvas.drawRect(l, t, l + f.width.toFloat(), t + f.height.toFloat(), framePaint)
        dotPaint.color = Color.BLACK
        for (s in ink.strokes) {
            val b = s.bounds
            canvas.drawRect(l + f.fit.screenX(b.left).toFloat(), t + f.fit.screenY(b.top).toFloat(),
                l + f.fit.screenX(b.right).toFloat().coerceAtLeast(f.fit.screenX(b.left).toFloat() + 1),
                t + f.fit.screenY(b.bottom).toFloat().coerceAtLeast(f.fit.screenY(b.top).toFloat() + 1), dotPaint)
        }
        val vis = viewport.visible(width.toDouble(), height.toDouble())
        canvas.drawRect(l + f.fit.screenX(vis.left).toFloat(), t + f.fit.screenY(vis.top).toFloat(),
            l + f.fit.screenX(vis.right).toFloat(), t + f.fit.screenY(vis.bottom).toFloat(), framePaint)
    }

    override fun onDetachedFromWindow() {
        removeCallbacks(hideMinimap)
        viewTreeObserver.removeOnGlobalLayoutListener(layoutWatch)
        super.onDetachedFromWindow()
    }

    companion object {
        const val FIT_PAD = 48.0
        const val TAP_SLOP_PX = 18f
        const val ERASE_RADIUS_PX = 22f
        const val LISTEN_RADIUS_PX = 24f
        const val HIGHLIGHT_FACTOR = 3.3f
        val MUTED = Color.rgb(170, 170, 170)
    }
}

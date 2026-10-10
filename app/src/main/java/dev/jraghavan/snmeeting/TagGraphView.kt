package dev.jraghavan.snmeeting

import android.annotation.SuppressLint
import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.DashPathEffect
import android.graphics.Paint
import android.graphics.Typeface
import android.view.MotionEvent
import android.view.ScaleGestureDetector
import android.view.View
import dev.jraghavan.snmeeting.model.TagGraph
import dev.jraghavan.snmeeting.model.TagGraphMath
import dev.jraghavan.snmeeting.model.Viewport
import kotlin.math.abs

/**
 * F20-FR7 / FR9: the Tag Graph, drawn still from the core's layout. Filled black circles sized
 * by recordings, lines between tags used together, names inside when they fit, the selected
 * tag ringed with a dashed line. No animation (NFR2): a pinch or a drag redraws once when the
 * finger lifts, and a tap selects. [interactive] off (Home's compact panel) keeps taps only.
 */
class TagGraphView(
    context: Context,
    private val density: Float,
    private val interactive: Boolean,
    private val onTap: (Int?) -> Unit,
) : View(context) {
    private var graph: TagGraph = TagGraph.EMPTY
    private var vp: Viewport? = null
    private var fitVp: Viewport? = null
    var selected: Int? = null
        set(v) { if (field != v) { field = v; invalidate() } }

    /** The smallest circle stays tappable: 48 dp across (24 on Home's small panel). */
    private val minR = (if (interactive) 24f else 12f) * density
    /** Names show only on circles at least this big (Home's panel: the larger ones). */
    private val labelR = (if (interactive) 0f else 22f) * density

    private val fill = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = Color.BLACK; style = Paint.Style.FILL }
    private val line = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = Color.BLACK; style = Paint.Style.STROKE; strokeCap = Paint.Cap.ROUND }
    private val ring = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.BLACK; style = Paint.Style.STROKE; strokeWidth = 3f * density
        pathEffect = DashPathEffect(floatArrayOf(10f * density, 6f * density), 0f)
    }
    private val inside = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.WHITE; typeface = Typeface.MONOSPACE; textAlign = Paint.Align.CENTER
    }
    private val beside = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = Color.BLACK; typeface = Typeface.MONOSPACE }

    /** A new graph: drawn fitted. The same nodes again keep the zoom and pan. */
    fun show(g: TagGraph) {
        val same = g.nodes == graph.nodes && g.edges == graph.edges
        graph = g
        if (!same) { vp = null; fitVp = null }
        invalidate()
    }

    /** "Fit": the whole graph again. */
    fun fit() { vp = null; invalidate() }

    /** ＋ / −: zoom about the middle. */
    fun zoomBy(factor: Double) {
        val v = vp ?: return
        vp = TagGraphMath.zoom(v, factor, width / 2.0, height / 2.0, fitVp ?: v)
        invalidate()
    }

    private fun maxR() = width / 8.0

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) { vp = null; fitVp = null }

    override fun onDraw(c: Canvas) {
        c.drawColor(Color.WHITE)
        val g = graph
        if (g.nodes.isEmpty() || width == 0) return
        val f = fitVp ?: TagGraphMath.fit(g, width, height, (minR + 8 * density).toInt()).also { fitVp = it }
        val v = vp ?: f.also { vp = it }
        val (min, max) = minR.toDouble() to maxR()
        TagGraphMath.visibleEdges(g, selected).forEach { e ->
            val a = g.nodes[e.a]; val b = g.nodes[e.b]
            line.strokeWidth = TagGraphMath.strokePx(e.shared, density * (if (interactive) 1.5 else 1.0)).toFloat()
            c.drawLine(v.x(a.x).toFloat(), v.y(a.y).toFloat(), v.x(b.x).toFloat(), v.y(b.y).toFloat(), line)
        }
        g.nodes.forEachIndexed { i, n ->
            val r = TagGraphMath.radiusPx(n.r, v, min, max).toFloat()
            val cx = v.x(n.x).toFloat(); val cy = v.y(n.y).toFloat()
            c.drawCircle(cx, cy, r, fill)
            if (i == selected) c.drawCircle(cx, cy, r + 7 * density, ring)
            if (r < labelR) return@forEachIndexed
            val text = TagGraphMath.label(n)
            inside.textSize = (r / 3f).coerceIn(11f * density, 20f * density)
            if (TagGraphMath.fitsInside(inside.measureText(text), r.toDouble())) {
                c.drawText(text, cx, cy + inside.textSize / 3f, inside)
            } else {
                beside.textSize = 13f * density
                c.drawText(text, cx + r + 6 * density, cy + beside.textSize / 3f, beside)
            }
        }
    }

    // A pinch or a drag moves the picture once, when the finger lifts (one e-ink refresh).
    private var downX = 0f
    private var downY = 0f
    private var moved = false
    private var pending: Viewport? = null
    private val scaler = ScaleGestureDetector(context, object : ScaleGestureDetector.SimpleOnScaleGestureListener() {
        override fun onScale(d: ScaleGestureDetector): Boolean {
            val v = pending ?: vp ?: return false
            pending = TagGraphMath.zoom(v, d.scaleFactor.toDouble(), d.focusX.toDouble(), d.focusY.toDouble(), fitVp ?: v)
            moved = true
            return true
        }
    })

    @SuppressLint("ClickableViewAccessibility") // a tap selects a circle; there is no click
    override fun onTouchEvent(e: MotionEvent): Boolean {
        if (interactive) scaler.onTouchEvent(e)
        when (e.actionMasked) {
            MotionEvent.ACTION_DOWN -> { downX = e.x; downY = e.y; moved = false; pending = null }
            MotionEvent.ACTION_MOVE -> if (interactive && !scaler.isInProgress && e.pointerCount == 1) {
                val slop = 12 * density
                if (moved || abs(e.x - downX) > slop || abs(e.y - downY) > slop) {
                    moved = true
                    val v = vp ?: return true
                    pending = TagGraphMath.pan(pending ?: v, (e.x - downX).toDouble(), (e.y - downY).toDouble())
                    downX = e.x; downY = e.y
                }
            }
            MotionEvent.ACTION_UP -> {
                if (moved) { pending?.let { vp = it }; pending = null; invalidate() }
                else vp?.let { v -> onTap(TagGraphMath.hit(graph, v, e.x.toDouble(), e.y.toDouble(), minR.toDouble(), maxR())) }
            }
            MotionEvent.ACTION_CANCEL -> { pending = null; moved = false }
        }
        return true
    }
}

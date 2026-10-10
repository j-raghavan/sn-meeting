package dev.jraghavan.snmeeting.notes

import android.annotation.SuppressLint
import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Path
import android.graphics.Typeface
import android.util.Log
import android.view.View

/**
 * DEVICE-UNVERIFIED (placement on the Manta). F17-FR14: the hints drawn over the Notes tab, as
 * sn-canvas shows them on an empty canvas: one redraw when shown, one when dismissed, no
 * animation. It never takes a touch. [targets] gives each control's arrow tip in this view's
 * pixels; [inkAt] says whether a box (px) would cover ink, and such a hint is left out, as is
 * one that would overlap another.
 */
@SuppressLint("ViewConstructor")
class HintsOverlay(
    context: Context,
    private val targets: () -> Map<String, Pair<Float, Float>>,
    private val inkAt: (Float, Float, Float, Float) -> Boolean,
    private val barOnLeft: () -> Boolean,
) : View(context) {
    private val dp = context.resources.displayMetrics.density
    private val font: Typeface = try {
        Typeface.createFromAsset(context.assets, "fonts/virgil.ttf")
    } catch (e: RuntimeException) {
        Log.w("Meetings", "hint font: ${e.javaClass.simpleName}"); Typeface.DEFAULT
    }
    private val ink = Color.rgb(85, 85, 85) // sn-canvas: the screen tints the hints gray
    private val text = Paint().apply { color = ink; typeface = font; textSize = 18 * dp; isAntiAlias = true }
    private val line = Paint().apply {
        color = ink; style = Paint.Style.STROKE; strokeWidth = 1.6f * dp
        strokeCap = Paint.Cap.ROUND; strokeJoin = Paint.Join.ROUND; isAntiAlias = true
    }
    private val path = Path()

    var showing = false
        private set

    fun show() { showing = true; invalidate() }

    fun hide() { if (showing) { showing = false; invalidate() } }

    @SuppressLint("ClickableViewAccessibility") // touches go through to the canvas and the bar
    override fun onTouchEvent(e: android.view.MotionEvent) = false

    override fun onDraw(canvas: Canvas) {
        if (!showing) return
        val tips = targets()
        val placed = ArrayList<Hints.Placed>()
        for (spec0 in Hints.ALL) {
            val tipPx = tips[spec0.id] ?: continue
            // A bar on the left: its captions go to the right of it.
            val spec = if (barOnLeft() && spec0.side == Hints.Side.LEFT) spec0.copy(side = Hints.Side.RIGHT) else spec0
            val textW = spec.lines.maxOf { text.measureText(it) } / dp
            val p = Hints.place(spec, textW.toDouble(), (tipPx.first / dp).toDouble() to (tipPx.second / dp).toDouble())
            val (l, t, r, b) = listOf(p.left, p.top, p.right, p.bottom).map { (it * dp).toFloat() }
            if (l < 0 || t < 0 || r > width || b > height) continue
            if (inkAt(l, t, r, b) || placed.any { it.meets(p) }) continue
            placed += p
            spec.lines.forEachIndexed { i, s ->
                canvas.drawText(s, l, t + ((i + 0.72) * Hints.LINE_DP * dp).toFloat(), text)
            }
            val (points, toward) = Hints.arrow(p.tail, p.tip)
            path.reset()
            points.forEachIndexed { i, (x, y) -> if (i == 0) path.moveTo((x * dp).toFloat(), (y * dp).toFloat()) else path.lineTo((x * dp).toFloat(), (y * dp).toFloat()) }
            Hints.head(p.tip, toward).forEach { (x, y) ->
                path.moveTo((p.tip.first * dp).toFloat(), (p.tip.second * dp).toFloat())
                path.lineTo((x * dp).toFloat(), (y * dp).toFloat())
            }
            canvas.drawPath(path, line)
        }
    }
}

package dev.jraghavan.snmeeting.notes

import android.annotation.SuppressLint
import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Path
import android.os.Handler
import android.os.Looper
import android.view.MotionEvent
import android.view.View
import dev.jraghavan.snmeeting.model.CoreJson
import dev.jraghavan.snmeeting.model.CoreResult
import dev.jraghavan.snmeeting.model.str
import java.io.ByteArrayOutputStream

/** F19-FR1 / FR9: the handwritten query, sized and read back. Pure, JVM-tested. */
object QueryStrip {
    const val MAX_W = 1568
    const val MAX_H = 200
    const val OFFLINE = "Can't read handwriting offline — type your search"

    /** A [w] × [h] strip scaled down (never up) to fit the provider's 1568 × 200. */
    fun fit(w: Int, h: Int): Pair<Int, Int> {
        if (w <= 0 || h <= 0) return 0 to 0
        val s = minOf(1.0, MAX_W.toDouble() / w, MAX_H.toDouble() / h)
        return maxOf(1, (w * s).toInt()) to maxOf(1, (h * s).toInt())
    }

    /** `nativeRecogniseQuery`'s reply: the text read, or null (offline, failed, nothing read). */
    fun text(json: String): String? = when (val r = CoreJson.obj(json) { it.str("text") }) {
        is CoreResult.Ok -> r.value.trim().ifEmpty { null }
        is CoreResult.Err -> null
    }
}

/**
 * DEVICE-UNVERIFIED (pen on the Manta). F19-FR1's ✎ ink strip on Search: the pen writes the
 * query (a finger does nothing), and [IDLE_MS] after the pen lifts the strip is drawn to a
 * PNG of at most 1568 × 200 px and handed to [onDone] for `nativeRecogniseQuery`.
 */
@SuppressLint("ViewConstructor")
class QueryInkView(context: Context, private val onDone: (ByteArray) -> Unit) : View(context) {
    private val strokes = ArrayList<FloatArray>()
    private val current = ArrayList<Float>()
    private val handler = Handler(Looper.getMainLooper())
    private val done = Runnable { if (strokes.isNotEmpty()) onDone(png()) }
    private val paint = Paint().apply {
        color = Color.BLACK; style = Paint.Style.STROKE; strokeWidth = 6f
        strokeCap = Paint.Cap.ROUND; strokeJoin = Paint.Join.ROUND; isAntiAlias = true
    }
    private val path = Path()

    fun clear() { strokes.clear(); current.clear(); handler.removeCallbacks(done); invalidate() }

    @SuppressLint("ClickableViewAccessibility") // a writing surface, not a button
    override fun onTouchEvent(e: MotionEvent): Boolean {
        if (e.getToolType(0) != MotionEvent.TOOL_TYPE_STYLUS) return false
        when (e.actionMasked) {
            MotionEvent.ACTION_DOWN -> { handler.removeCallbacks(done); current.clear(); current += e.x; current += e.y }
            MotionEvent.ACTION_MOVE -> {
                for (h in 0 until e.historySize) { current += e.getHistoricalX(h); current += e.getHistoricalY(h) }
                current += e.x; current += e.y
                invalidate()
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                if (current.size >= 2) strokes += current.toFloatArray()
                current.clear()
                handler.postDelayed(done, IDLE_MS)
                invalidate()
            }
        }
        return true
    }

    private fun draw(c: Canvas, s: Float) {
        for (xy in strokes + listOf(current.toFloatArray())) {
            if (xy.size < 2) continue
            path.reset(); path.moveTo(xy[0] * s, xy[1] * s)
            var i = 2
            while (i + 1 < xy.size) { path.lineTo(xy[i] * s, xy[i + 1] * s); i += 2 }
            if (xy.size == 2) path.lineTo(xy[0] * s + 0.5f, xy[1] * s)
            c.drawPath(path, paint)
        }
    }

    override fun onDraw(canvas: Canvas) {
        canvas.drawColor(Color.WHITE)
        draw(canvas, 1f)
    }

    private fun png(): ByteArray {
        val (w, h) = QueryStrip.fit(width, height)
        val bmp = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
        val c = Canvas(bmp)
        c.drawColor(Color.WHITE)
        draw(c, w.toFloat() / maxOf(1, width))
        val out = ByteArrayOutputStream()
        bmp.compress(Bitmap.CompressFormat.PNG, 100, out)
        bmp.recycle()
        return out.toByteArray()
    }

    override fun onDetachedFromWindow() { handler.removeCallbacks(done); super.onDetachedFromWindow() }

    companion object {
        /** The pen rests this long before the query is read. */
        const val IDLE_MS = 1_500L
    }
}

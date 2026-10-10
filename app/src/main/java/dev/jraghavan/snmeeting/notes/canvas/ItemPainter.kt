// Adapted from sn-canvas `android/app/src/main/java/com/sncanvas/canvas/ElementPainter.kt`,
// `TextPainter.kt`, `ShapeDrawing.kt` and `AndroidTextMeasurer.kt` (MIT License, Copyright (c)
// 2026 JRaghavan; notice in LICENSES-3RDPARTY.md): package renamed, fed by the core's
// resolved items (`nativeCanvasForDraw`) instead of the React Native model.
package dev.jraghavan.snmeeting.notes.canvas

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.DashPathEffect
import android.graphics.LinearGradient
import android.graphics.Paint
import android.graphics.Path
import android.graphics.PointF
import android.graphics.RectF
import android.graphics.Shader
import android.graphics.Typeface
import android.text.Layout
import android.text.StaticLayout
import android.text.TextPaint
import dev.jraghavan.snmeeting.notes.Viewport
import kotlin.math.atan2
import kotlin.math.cos
import kotlin.math.sin

/** Where an image item's pixels come from (F21-FR11): [ImageCache] on the device. */
fun interface ImageSource {
    fun bitmap(image: DrawImage): Bitmap?

    companion object {
        val NONE = ImageSource { null }
    }
}

/** The text layout every text box, note and table cell is measured and drawn with (F21-FR9). */
object TextLayout {
    /** The inset between a box's edge and its text, shared by measuring and drawing. */
    const val PADDING = 8.0
    const val LINE_SPACING = 1.2f
    /** Table text is set smaller than a text box's, so cells stay compact. */
    const val TABLE_FONT_SCALE = 0.75

    /** The Inkwell serif (F21-FR9: the editor and the drawn text match). */
    val typeface: Typeface = Typeface.SERIF

    fun layout(text: String, paint: TextPaint, width: Float): StaticLayout =
        StaticLayout.Builder.obtain(text, 0, text.length, paint, width.toInt().coerceAtLeast(1))
            .setAlignment(Layout.Alignment.ALIGN_NORMAL)
            .setLineSpacing(0f, LINE_SPACING)
            .setIncludePad(false)
            .build()

    private val measurePaint = TextPaint(TextPaint.ANTI_ALIAS_FLAG).apply { typeface = TextLayout.typeface }

    /** How tall [text] is in world units when wrapped to [width] at [fontSize], padding included. */
    @Synchronized
    fun height(text: String, width: Double, fontSize: Double): Double {
        measurePaint.textSize = fontSize.toFloat()
        val inner = (width - 2 * PADDING).coerceAtLeast(1.0)
        return layout(text, measurePaint, inner.toFloat()).height.toDouble() + 2 * PADDING
    }
}

/**
 * Paints one item in its style (F21-FR6/FR8–FR11): shapes and connectors with colour through
 * a [StylePalette], opacity, fill (none, semi, solid, hatched or gradient), dash (hand-drawn,
 * dashed, dotted or solid) and size; images from [images], framed by their outline or none;
 * text, sticky notes and table cells; and an unknown type as a dashed placeholder box. Its
 * paints are reconfigured for every item, so one painter stays on one thread.
 */
class ItemPainter(private val images: ImageSource) {
    /** How to paint: through which viewport and palette, leaving out the text being edited. */
    data class Context(val view: Viewport, val palette: StylePalette, val editing: Pair<String, Int?>? = null)

    private val strokePaint = Paint().apply {
        style = Paint.Style.STROKE; strokeCap = Paint.Cap.ROUND; strokeJoin = Paint.Join.ROUND; isAntiAlias = true
    }
    private val fillPaint = Paint().apply { style = Paint.Style.FILL; isAntiAlias = true }
    private val hatchPaint = Paint().apply { style = Paint.Style.STROKE; strokeWidth = HATCH_WIDTH_PX; isAntiAlias = true }
    private val missingPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.FILL; color = MISSING_IMAGE_FILL }
    private val placeholderPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE; color = Color.GRAY; strokeWidth = 2f; pathEffect = DashPathEffect(floatArrayOf(10f, 8f), 0f)
    }
    private val arrowheadPaint = Paint()
    private val imagePaint = Paint(Paint.FILTER_BITMAP_FLAG or Paint.ANTI_ALIAS_FLAG)
    private val textPaint = TextPaint(Paint.ANTI_ALIAS_FLAG).apply { typeface = TextLayout.typeface }
    private val notePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.FILL }
    private val gridPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.STROKE }

    fun draw(canvas: Canvas, item: DrawItem, c: Context) {
        val image = item.image
        when {
            !item.known -> drawPlaceholder(canvas, item, c)
            item.hasEnds -> drawConnector(canvas, item, c)
            image != null -> drawImage(canvas, item, image, c)
            item.type == "text" || item.type == "note" -> drawTextItem(canvas, item, c)
            else -> {
                drawShape(canvas, item, c)
                if (item.table != null) drawTableCells(canvas, item, c)
            }
        }
    }

    private fun bounds(item: DrawItem, v: Viewport): RectF = RectF(
        v.screenX(item.x).toFloat(), v.screenY(item.y).toFloat(),
        v.screenX(item.x + item.width).toFloat(), v.screenY(item.y + item.height).toFloat(),
    )

    private inline fun rotated(canvas: Canvas, item: DrawItem, b: RectF, draw: () -> Unit) {
        if (item.rotation == 0.0) { draw(); return }
        canvas.save()
        canvas.rotate(Math.toDegrees(item.rotation).toFloat(), b.centerX(), b.centerY())
        draw()
        canvas.restore()
    }

    private fun screenPath(points: List<Pt>, v: Viewport, closed: Boolean): Path = Path().apply {
        points.forEachIndexed { i, p ->
            val x = v.screenX(p.x).toFloat(); val y = v.screenY(p.y).toFloat()
            if (i == 0) moveTo(x, y) else lineTo(x, y)
        }
        if (closed) close()
    }

    private fun wobbly(points: List<Pt>, closed: Boolean, item: DrawItem): List<Pt> =
        ShapeOutline.handDrawn(points, closed, item.id.hashCode(), item.style.size.strokeWidth * WOBBLE_PER_STROKE)

    private fun shapePath(item: DrawItem, b: RectF, v: Viewport): Path = when {
        item.style.dash == DashStyle.DRAW -> screenPath(wobbly(ShapeOutline.of(item), true, item), v, true)
        item.type == "ellipse" -> Path().apply { addOval(b, Path.Direction.CW) }
        else -> Path().apply { addRect(b, Path.Direction.CW) }
    }

    private fun drawPlaceholder(canvas: Canvas, item: DrawItem, c: Context) {
        val b = bounds(item, c.view)
        rotated(canvas, item, b) { canvas.drawRect(b, placeholderPaint) }
    }

    private fun drawShape(canvas: Canvas, item: DrawItem, c: Context) {
        val b = bounds(item, c.view)
        val outline = shapePath(item, b, c.view)
        rotated(canvas, item, b) {
            drawFill(canvas, outline, b, item.style, c.palette)
            drawOutline(canvas, outline, item.style, c)
        }
    }

    private fun drawImage(canvas: Canvas, item: DrawItem, image: DrawImage, c: Context) {
        val b = bounds(item, c.view)
        rotated(canvas, item, b) {
            val bmp = images.bitmap(image)
            if (bmp == null) {
                canvas.drawRect(b, missingPaint)
                hatchPaint.color = Color.GRAY; hatchPaint.alpha = 255
                canvas.drawLine(b.left, b.top, b.right, b.bottom, hatchPaint)
                canvas.drawLine(b.left, b.bottom, b.right, b.top, hatchPaint)
            } else {
                imagePaint.alpha = item.style.alpha
                canvas.drawBitmap(bmp, null, b, imagePaint)
            }
            drawOutline(canvas, shapePath(item, b, c.view), item.style, c)
        }
    }

    private fun drawOutline(canvas: Canvas, outline: Path, style: ShapeStyle, c: Context) {
        if (style.dash == DashStyle.NONE) return
        configureStroke(style, c.palette, style.strokeWidthPx(c.view.zoom))
        canvas.drawPath(outline, strokePaint)
    }

    private fun drawConnector(canvas: Canvas, item: DrawItem, c: Context) {
        val e = item.ends ?: return
        val start = Pt(e[0], e[1]); val end = Pt(e[2], e[3])
        val style = item.style
        val width = style.strokeWidthPx(c.view.zoom)
        val points = if (style.dash == DashStyle.DRAW) wobbly(listOf(start, end), false, item) else listOf(start, end)
        configureStroke(style, c.palette, width)
        canvas.drawPath(screenPath(points, c.view, false), strokePaint)
        if (item.type == "arrow") {
            arrowheadPaint.set(strokePaint)
            arrowheadPaint.pathEffect = null
            val length = (width * ARROWHEAD_PER_STROKE).coerceIn(MIN_ARROWHEAD_PX, MAX_ARROWHEAD_PX)
            val from = PointF(c.view.screenX(start.x).toFloat(), c.view.screenY(start.y).toFloat())
            val last = points.last()
            val to = PointF(c.view.screenX(last.x).toFloat(), c.view.screenY(last.y).toFloat())
            val angle = atan2((to.y - from.y).toDouble(), (to.x - from.x).toDouble())
            for (side in listOf(angle - ARROWHEAD_SPREAD_RAD, angle + ARROWHEAD_SPREAD_RAD)) {
                canvas.drawLine(to.x, to.y, to.x - (length * cos(side)).toFloat(), to.y - (length * sin(side)).toFloat(), arrowheadPaint)
            }
        }
    }

    private fun rampShader(b: RectF, color: Int): LinearGradient {
        val line = ShapeOutline.rampLine(b.top, b.bottom)
        return LinearGradient(line[0], line[1], line[2], line[3], color, StylePalette.fadeToNothing(color), Shader.TileMode.CLAMP)
    }

    private fun drawFill(canvas: Canvas, outline: Path, b: RectF, style: ShapeStyle, palette: StylePalette) {
        if (style.fill == FillStyle.NONE) return
        val tint = palette.fillTint(style.fill, style.color)
        fillPaint.color = tint
        fillPaint.shader = if (style.fill.ramps) rampShader(b, tint) else null
        fillPaint.alpha = style.alpha
        canvas.drawPath(outline, fillPaint)
        if (style.fill == FillStyle.PATTERN) {
            hatchPaint.color = palette.patternLine(style.color)
            hatchPaint.alpha = style.alpha
            canvas.save()
            canvas.clipPath(outline)
            var offset = -b.height()
            while (offset < b.width()) {
                canvas.drawLine(b.left + offset, b.bottom, b.left + offset + b.height(), b.top, hatchPaint)
                offset += HATCH_SPACING_PX
            }
            canvas.restore()
        }
    }

    private fun configureStroke(style: ShapeStyle, palette: StylePalette, width: Float) {
        strokePaint.color = palette.stroke(style.color)
        strokePaint.alpha = style.alpha
        strokePaint.strokeWidth = width
        strokePaint.pathEffect = when (style.dash) {
            DashStyle.DASHED -> DashPathEffect(floatArrayOf(width * DASH_LENGTH, width * DASH_GAP), 0f)
            DashStyle.DOTTED -> DashPathEffect(floatArrayOf(0f, width * DOT_GAP), 0f)
            DashStyle.DRAW, DashStyle.SOLID, DashStyle.NONE -> null
        }
    }

    // ---- text, notes and tables (sn-canvas TextPainter) --------------------------------------

    private fun drawTextItem(canvas: Canvas, item: DrawItem, c: Context) {
        val zoom = c.view.zoom
        val b = bounds(item, c.view)
        val isNote = item.type == "note"
        rotated(canvas, item, b) {
            if (isNote) {
                val tint = c.palette.fillTint(item.style.fill, item.style.color)
                notePaint.color = tint
                notePaint.shader = if (item.style.fill.ramps) rampShader(b, tint) else null
                notePaint.alpha = item.style.alpha
                canvas.drawRect(b, notePaint)
            }
            if (c.editing?.first != item.id) {
                val color = c.palette.stroke(if (isNote) StyleColor.BLACK else item.style.color)
                drawText(canvas, item.text.orEmpty(), inset(b, zoom), item.style.size.fontSize * zoom, withAlpha(color, item.style.alpha))
            }
        }
    }

    private fun drawTableCells(canvas: Canvas, item: DrawItem, c: Context) {
        val table = item.table ?: return
        val zoom = c.view.zoom
        val b = bounds(item, c.view)
        val color = withAlpha(c.palette.stroke(item.style.color), item.style.alpha)
        val columnWidth = b.width() / table.cols
        val rowTops = table.rowHeights.runningFold(0.0, Double::plus).map { (it * zoom).toFloat() }
        gridPaint.color = color
        gridPaint.strokeWidth = (item.style.strokeWidthPx(zoom) / 2).coerceAtLeast(1f)
        rotated(canvas, item, b) {
            for (col in 1 until table.cols) {
                canvas.drawLine(b.left + col * columnWidth, b.top, b.left + col * columnWidth, b.top + rowTops.last(), gridPaint)
            }
            for (row in 1 until table.rows) canvas.drawLine(b.left, b.top + rowTops[row], b.right, b.top + rowTops[row], gridPaint)
            for (index in table.cells.indices) {
                if (c.editing == item.id to index) continue
                val row = index / table.cols
                val left = b.left + index % table.cols * columnWidth
                val cell = RectF(left, b.top + rowTops[row], left + columnWidth, b.top + rowTops[row + 1])
                drawText(canvas, table.cells[index], inset(cell, zoom), item.style.size.fontSize * TextLayout.TABLE_FONT_SCALE * zoom, color)
            }
        }
    }

    private fun drawText(canvas: Canvas, text: String, box: RectF, fontSizePx: Double, color: Int) {
        if (text.isEmpty() || box.width() <= 0f) return
        textPaint.textSize = fontSizePx.toFloat()
        textPaint.color = color
        val layout = TextLayout.layout(text, textPaint, box.width())
        canvas.save()
        canvas.translate(box.left, box.top)
        layout.draw(canvas)
        canvas.restore()
    }

    private fun inset(box: RectF, zoom: Double): RectF {
        val pad = (TextLayout.PADDING * zoom).toFloat()
        return RectF(box.left + pad, box.top + pad, box.right - pad, box.bottom - pad)
    }

    private fun withAlpha(color: Int, alpha: Int): Int = (color and 0xFFFFFF) or (alpha shl 24)

    private companion object {
        const val WOBBLE_PER_STROKE = 0.6
        const val ARROWHEAD_PER_STROKE = 8f
        const val MIN_ARROWHEAD_PX = 10f
        const val MAX_ARROWHEAD_PX = 60f
        const val ARROWHEAD_SPREAD_RAD = Math.PI / 7
        const val DASH_LENGTH = 4f
        const val DASH_GAP = 3f
        const val DOT_GAP = 2.5f
        const val HATCH_SPACING_PX = 12f
        const val HATCH_WIDTH_PX = 2f
        const val MISSING_IMAGE_FILL = 0xFFE6E6E6.toInt()
    }
}

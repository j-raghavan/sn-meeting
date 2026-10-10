// Adapted from sn-canvas `android/app/src/main/java/com/sncanvas/canvas/PdfExport.kt` (MIT
// License, Copyright (c) 2026 JRaghavan; notice in LICENSES-3RDPARTY.md): one page per canvas,
// planned by the core (`nativeCanvasPdfPlan`).
package dev.jraghavan.snmeeting.notes.canvas

import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Path
import android.graphics.pdf.PdfDocument
import dev.jraghavan.snmeeting.model.arr
import dev.jraghavan.snmeeting.model.int
import dev.jraghavan.snmeeting.model.str
import dev.jraghavan.snmeeting.notes.DrawEntry
import dev.jraghavan.snmeeting.notes.DrawStroke
import dev.jraghavan.snmeeting.notes.InkView
import dev.jraghavan.snmeeting.notes.PressureWidth
import dev.jraghavan.snmeeting.notes.Viewport
import org.json.JSONObject
import java.io.File
import java.io.FileOutputStream
import java.io.IOException

/**
 * F21-FR14: the notes PDF the core planned: one page per canvas, each fitted to its content,
 * in **true colour** (`StylePalette.TRUE_COLOR`), strokes at their pressure widths, the
 * highlighter at its opacity, and shapes, text, tables, images and link badges as on screen;
 * no recording times. Drawn off the UI thread into its own document, written to the plan's
 * tmp file in the meeting's folder, synced, then renamed over any earlier export.
 */
object PdfExport {
    /** One planned page: its size in points, the transform its content is drawn through. */
    data class Page(val widthPt: Int, val heightPt: Int, val view: Viewport, val content: List<DrawEntry>)

    data class Plan(val path: File, val tmp: File, val images: File?, val pages: List<Page>) {
        companion object {
            fun from(o: JSONObject): Plan? {
                val path = o.str("path") ?: return null
                val tmp = o.str("tmpPath") ?: return null
                val pages = o.arr("pages") ?: return null
                return Plan(File(path), File(tmp), o.str("imagesDir")?.let(::File), (0 until pages.length()).mapNotNull { i ->
                    val p = pages.optJSONObject(i) ?: return@mapNotNull null
                    val v = p.arr("viewport") ?: return@mapNotNull null
                    if (v.length() != 3) return@mapNotNull null
                    val entries = InkView.from(JSONObject().put("entries", p.arr("content"))).entries
                    Page(p.int("widthPt") ?: return@mapNotNull null, p.int("heightPt") ?: return@mapNotNull null,
                        Viewport(v.optDouble(0), v.optDouble(1), v.optDouble(2)), entries)
                })
            }
        }
    }

    /** Write [plan]'s PDF; null when done, else why it failed ("Storage full"). */
    fun write(plan: Plan): String? {
        val images = ImageCache().apply { folder = plan.images }
        val painter = ItemPainter(images)
        val doc = PdfDocument()
        try {
            plan.pages.forEachIndexed { i, p ->
                val page = doc.startPage(PdfDocument.PageInfo.Builder(p.widthPt, p.heightPt, i + 1).create())
                page.canvas.drawColor(Color.WHITE)
                drawPage(page.canvas, p, painter)
                doc.finishPage(page)
            }
            plan.tmp.parentFile?.mkdirs()
            FileOutputStream(plan.tmp).use { out -> doc.writeTo(out); out.fd.sync() }
            if (!plan.tmp.renameTo(plan.path)) {
                plan.tmp.delete()
                return "The PDF can't be saved"
            }
            return null
        } catch (e: IOException) {
            plan.tmp.delete()
            return if (e.message.orEmpty().contains("ENOSPC") || e.message.orEmpty().contains("No space")) "Storage full"
            else "The PDF can't be saved"
        } finally {
            doc.close()
        }
    }

    private val ink = Paint().apply { style = Paint.Style.STROKE; strokeCap = Paint.Cap.ROUND; strokeJoin = Paint.Join.ROUND; isAntiAlias = true }
    private val dot = Paint().apply { style = Paint.Style.FILL; isAntiAlias = true }

    private fun drawPage(c: Canvas, p: Page, painter: ItemPainter) {
        val ctx = ItemPainter.Context(p.view, StylePalette.TRUE_COLOR)
        for (e in p.content) {
            when (e) {
                is DrawEntry.Ink -> drawStroke(c, e.stroke, p.view)
                is DrawEntry.Thing -> painter.draw(c, e.item, ctx)
            }
        }
        // F21-FR14: link badges as on screen (no link URLs in the file).
        for (e in p.content) {
            if (!Badges.linked(e)) continue
            val b = Badges.point(e, p.view.zoom)
            val x = p.view.screenX(b.x).toFloat(); val y = p.view.screenY(b.y).toFloat()
            c.drawCircle(x, y, BADGE_PT, badgeFill); c.drawCircle(x, y, BADGE_PT, badgeRing)
            c.drawLine(x - BADGE_PT / 2, y + BADGE_PT / 2, x + BADGE_PT / 2, y - BADGE_PT / 2, badgeRing)
        }
    }

    private const val BADGE_PT = 9f
    private val badgeFill = Paint().apply { style = Paint.Style.FILL; color = Color.WHITE; isAntiAlias = true }
    private val badgeRing = Paint().apply { style = Paint.Style.STROKE; color = Color.BLACK; strokeWidth = 1.5f; isAntiAlias = true }

    /** A stroke in its true colour: the style's, or `.inkbin`'s RGBA (the highlighter's band). */
    @Synchronized
    private fun drawStroke(c: Canvas, s: DrawStroke, v: Viewport) {
        val color = s.style?.let { (StylePalette.TRUE_COLOR.stroke(it.color) and 0xFFFFFF) or (s.argb and 0xFF000000.toInt()) } ?: s.argb
        ink.color = color; dot.color = color
        val z = v.zoom.toFloat()
        val pr = s.pressure
        if (s.pts.size == 2) {
            c.drawCircle(v.screenX(s.pts[0].toDouble()).toFloat(), v.screenY(s.pts[1].toDouble()).toFloat(),
                maxOf(s.width * z, 0.5f) / 2, dot)
            return
        }
        if (pr != null) {
            var k = 0
            while (k + 3 < s.pts.size) {
                ink.strokeWidth = PressureWidth.of(s.width, (pr[k / 2] + pr[k / 2 + 1]) / 2) * z
                c.drawLine(v.screenX(s.pts[k].toDouble()).toFloat(), v.screenY(s.pts[k + 1].toDouble()).toFloat(),
                    v.screenX(s.pts[k + 2].toDouble()).toFloat(), v.screenY(s.pts[k + 3].toDouble()).toFloat(), ink)
                k += 2
            }
            return
        }
        ink.strokeWidth = s.width * z
        val path = Path()
        path.moveTo(v.screenX(s.pts[0].toDouble()).toFloat(), v.screenY(s.pts[1].toDouble()).toFloat())
        var i = 2
        while (i + 1 < s.pts.size) { path.lineTo(v.screenX(s.pts[i].toDouble()).toFloat(), v.screenY(s.pts[i + 1].toDouble()).toFloat()); i += 2 }
        c.drawPath(path, ink)
    }
}

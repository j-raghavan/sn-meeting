// Adapted from sn-canvas `android/app/src/main/java/com/sncanvas/canvas/TableElements.kt`
// (`cellAt`, `cellRect`, `rowEdgeAt`; MIT License, Copyright (c) 2026 JRaghavan; notice in
// LICENSES-3RDPARTY.md), on the row heights the core resolved.
package dev.jraghavan.snmeeting.notes.canvas

import dev.jraghavan.snmeeting.notes.WorldRect
import kotlin.math.abs

/** F21-FR10: where a table's cells and row edges are, in its unrotated frame. JVM-tested. */
object TableCells {
    /** The (row, col) under a world point, or null outside the table. */
    fun at(item: DrawItem, x: Double, y: Double): Pair<Int, Int>? {
        val t = item.table ?: return null
        val l = item.toLocal(x, y)
        if (l.x < item.x || l.y < item.y || l.x > item.x + item.width) return null
        var bottom = item.y
        val row = t.rowHeights.indexOfFirst { bottom += it; l.y <= bottom }.takeIf { it >= 0 } ?: return null
        val col = ((l.x - item.x) / (item.width / t.cols)).toInt().coerceIn(0, t.cols - 1)
        return row to col
    }

    /** A cell's box in the table's unrotated frame. */
    fun rect(item: DrawItem, row: Int, col: Int): WorldRect? {
        val t = item.table ?: return null
        if (row !in 0 until t.rows || col !in 0 until t.cols) return null
        val w = item.width / t.cols
        val top = item.y + t.rowHeights.take(row).sum()
        return WorldRect(item.x + col * w, top, item.x + (col + 1) * w, top + t.rowHeights[row])
    }

    /** The row whose bottom edge is within [tolerance] world units of a point, or null. */
    fun rowEdge(item: DrawItem, x: Double, y: Double, tolerance: Double): Int? {
        val t = item.table ?: return null
        val l = item.toLocal(x, y)
        if (l.x < item.x || l.x > item.x + item.width) return null
        var bottom = item.y
        return t.rowHeights.indexOfFirst { bottom += it; abs(bottom - l.y) <= tolerance }.takeIf { it >= 0 }
    }

    /** The height row [row] takes when its bottom edge is dragged to a world point. */
    fun rowHeightAt(item: DrawItem, row: Int, x: Double, y: Double): Double {
        val t = item.table ?: return 0.0
        return item.toLocal(x, y).y - (item.y + t.rowHeights.take(row).sum())
    }
}

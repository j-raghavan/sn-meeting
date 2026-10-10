package dev.jraghavan.snmeeting.notes.canvas

import dev.jraghavan.snmeeting.notes.WorldRect
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class TableCellsTest { // F21-FR10: cells and row edges on the core's row heights
    private val t = DrawItem("i:t", "t", "table", true, 0.0, 0.0, 320.0, 178.0, 0.0, null,
        table = DrawTable(2, 2, listOf("a", "b", "c", "d"), listOf(130.0, 48.0)))

    @Test fun cellsAndRowEdges() {
        assertEquals(1 to 1, TableCells.at(t, 170.0, 140.0))
        assertEquals(0 to 0, TableCells.at(t, 1.0, 1.0))
        assertNull(TableCells.at(t, -1.0, 10.0))
        assertNull(TableCells.at(t, 10.0, 500.0))
        assertEquals(WorldRect(160.0, 130.0, 320.0, 178.0), TableCells.rect(t, 1, 1))
        assertNull(TableCells.rect(t, 2, 0))
        assertEquals(0, TableCells.rowEdge(t, 50.0, 133.0, 6.0))
        assertNull(TableCells.rowEdge(t, 50.0, 60.0, 6.0))
        assertNull(TableCells.rowEdge(t, -50.0, 130.0, 6.0))
        assertEquals(150.0, TableCells.rowHeightAt(t, 1, 0.0, 280.0), 1e-9)
        assertNull(TableCells.at(t.copy(table = null), 1.0, 1.0))
    }
}

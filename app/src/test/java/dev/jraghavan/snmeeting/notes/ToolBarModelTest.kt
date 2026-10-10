package dev.jraghavan.snmeeting.notes

import dev.jraghavan.snmeeting.notes.ToolBarModel.Item
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ToolBarModelTest { // F17-FR2, F17-AC1 / AC1b
    private val recording = ToolBarModel.State(NoteTool.PEN, collapsed = false, recording = true, canUndo = true,
        timer = "0:12:41", cost = "≈ \$0.08 so far")

    @Test fun expandedWhileRecordingHasEveryToolThenMarkAboveStop() {
        val items = ToolBarModel.items(recording)
        val tools = items.filterIsInstance<Item.Tool>().map { it.tool }
        assertEquals(listOf(NoteTool.PEN, NoteTool.HIGHLIGHTER, NoteTool.ERASER, NoteTool.LASSO, NoteTool.LISTEN), tools)
        assertTrue(items.contains(Item.Undo(true)) && items.contains(Item.Redo(false)) && items.contains(Item.Options(false)))
        assertEquals(Item.Tool(NoteTool.LISTEN, selected = false, enabled = false), items.first { it is Item.Tool && it.tool == NoteTool.LISTEN })
        assertEquals(listOf(Item.Gap, Item.Timer("0:12:41"), Item.Cost("≈ \$0.08 so far"), Item.Mark(ToolBarModel.MARK), Item.Stop), items.takeLast(5))
        assertEquals(Item.Handle(false), items.first())
    }

    @Test fun collapsedKeepsTheTimerMarkAndStop() {
        val items = ToolBarModel.items(recording.copy(collapsed = true, tool = NoteTool.HIGHLIGHTER, marked = "Marked 0:12:41"))
        assertEquals(
            listOf(Item.Handle(true), Item.Tool(NoteTool.HIGHLIGHTER, true, true), Item.Gap,
                Item.Timer("0:12:41"), Item.Mark("Marked 0:12:41"), Item.Stop),
            items,
        )
    }

    @Test fun notRecordingThereIsPlayAndNoStop() {
        val items = ToolBarModel.items(ToolBarModel.State(NoteTool.LISTEN, collapsed = false, recording = false,
            timer = "0:03:10 / 0:41:00", playing = true, canPlay = true, focus = true))
        assertFalse(items.contains(Item.Stop))
        assertFalse(items.any { it is Item.Mark })
        assertEquals(listOf(Item.Timer("0:03:10 / 0:41:00"), Item.Play(playing = true, enabled = true)), items.takeLast(2))
        assertEquals(Item.Tool(NoteTool.LISTEN, selected = true, enabled = true), items.first { it is Item.Tool && it.tool == NoteTool.LISTEN })
        assertTrue(items.contains(Item.Options(true)))
    }

    @Test fun theTopBarShowsOnlyTheTimerAndOnlyWhileRecording() { // F17-FR1 (device finding 2026-10-09)
        assertEquals("●  12:41", TopBar.right(761_000))
        assertEquals("●  1:02:10", TopBar.right(3_730_000))
        assertEquals("", TopBar.right(null))
    }
}

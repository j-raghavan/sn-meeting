package dev.jraghavan.snmeeting.notes

import dev.jraghavan.snmeeting.notes.ToolBarModel.Act
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
        assertEquals(listOf(NoteTool.SELECT, NoteTool.PEN, NoteTool.HIGHLIGHTER, NoteTool.ERASER, NoteTool.LASSO, NoteTool.LISTEN), tools)
        assertTrue(items.contains(Item.Undo(true)) && items.contains(Item.Redo(false)) && items.contains(Item.More(false)))
        // F21-FR2: top to bottom.
        assertEquals(
            listOf(Item.Handle(false), Item.Tool(NoteTool.SELECT, false, true), Item.Tool(NoteTool.PEN, true, true),
                Item.Tool(NoteTool.HIGHLIGHTER, false, true), Item.Tool(NoteTool.ERASER, false, true), Item.Tool(NoteTool.LASSO, false, true),
                Item.Shapes(NoteTool.RECTANGLE, selected = false, open = false), Item.Insert(selected = false, open = false),
                Item.Style(dev.jraghavan.snmeeting.notes.canvas.ShapeStyle.DEFAULT, open = false), Item.Undo(true), Item.Redo(false),
                Item.Tool(NoteTool.LISTEN, false, false), Item.More(false), Item.Help, Item.Gap),
            items.take(15),
        )
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
        assertTrue(items.contains(Item.More(false)))
    }

    @Test fun theShapesButtonShowsTheShapeInUseOrTheLastOne() { // F21-FR2
        val s = recording.copy(tool = NoteTool.ELLIPSE, lastShape = NoteTool.ARROW, open = ToolBarModel.Panel.SHAPES)
        assertTrue(ToolBarModel.items(s).contains(Item.Shapes(NoteTool.ELLIPSE, selected = true, open = true)))
        val t = recording.copy(tool = NoteTool.NOTE, lastShape = NoteTool.ARROW)
        val items = ToolBarModel.items(t)
        assertTrue(items.contains(Item.Shapes(NoteTool.ARROW, selected = false, open = false)))
        assertTrue(items.contains(Item.Insert(selected = true, open = false)))
    }

    @Test fun aSelectionReplacesTheToolsWithItsActions() { // F21-FR2
        val one = ToolBarModel.items(recording.copy(selection = ToolBarModel.Selection(1)))
        assertFalse(one.any { it is Item.Tool || it is Item.Shapes })
        val acts = one.filterIsInstance<Item.Action>()
        assertEquals(listOf(Act.DELETE, Act.DUPLICATE, Act.GROUP, Act.FRONT, Act.BACK, Act.LINK, Act.STYLE, Act.DONE), acts.map { it.act })
        assertFalse("one thing can't be grouped", acts.first { it.act == Act.GROUP }.enabled)
        val group = ToolBarModel.items(recording.copy(selection = ToolBarModel.Selection(3, grouped = true, tableCell = true)))
        val names = group.filterIsInstance<Item.Action>().map { it.act }
        assertTrue(Act.UNGROUP in names && Act.GROUP !in names && Act.TABLE in names)
        assertEquals(listOf(Item.Gap, Item.Timer("0:12:41"), Item.Cost("≈ \$0.08 so far"), Item.Mark(ToolBarModel.MARK), Item.Stop), group.takeLast(5))
        assertTrue(ToolBarModel.items(recording.copy(selection = ToolBarModel.Selection(0))).any { it is Item.Tool })
    }

    @Test fun theMoreMenuOffersWhatApplies() { // F21-FR12
        assertEquals(listOf(MoreMenu.Entry.ZOOM_FIT, MoreMenu.Entry.ZOOM_100, MoreMenu.Entry.FOCUS_ON, MoreMenu.Entry.REFRESH),
            MoreMenu.items(MoreMenu.State()))
        val all = MoreMenu.items(MoreMenu.State(selection = true, tableCell = true, hasContent = true, canRestore = true, focus = true,
            canvases = true, export = true))
        assertEquals(MoreMenu.Entry.entries - MoreMenu.Entry.FOCUS_ON, all)
    }

    @Test fun theTopBarShowsOnlyTheTimerAndOnlyWhileRecording() { // F17-FR1 (device finding 2026-10-09)
        assertEquals("●  12:41", TopBar.right(761_000))
        assertEquals("●  1:02:10", TopBar.right(3_730_000))
        assertEquals("", TopBar.right(null))
    }

    @Test fun theBarIsOneButtonWideAndItsLabelsStack() { // owner, 2026-10-09: the bar was ~130 dp
        assertTrue(DockedToolBar.BAR_DP <= 72 && DockedToolBar.BUTTON_DP in 52..56)
        assertTrue(DockedToolBar.BUTTON_DP >= DockedToolBar.TOUCH_DP && DockedToolBar.BAR_DP > DockedToolBar.BUTTON_DP)
        assertEquals("◆\nMark", DockedToolBar.stacked(ToolBarModel.MARK))
        assertEquals("■\nStop", DockedToolBar.stacked("■  Stop"))
        assertEquals("Marked\n0:12:41", DockedToolBar.stacked("Marked 0:12:41"))
    }
}

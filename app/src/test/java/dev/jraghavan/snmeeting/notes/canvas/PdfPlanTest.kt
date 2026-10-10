package dev.jraghavan.snmeeting.notes.canvas

import dev.jraghavan.snmeeting.notes.NoteTool
import dev.jraghavan.snmeeting.notes.ToolBarModel
import dev.jraghavan.snmeeting.notes.Viewport
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class PdfPlanTest { // F21-FR14: the core's plan, read by the shell
    @Test fun aPageAPerCanvasWithItsContent() {
        val plan = PdfExport.Plan.from(JSONObject("""{"path":"/s/meeting-1.notes.pdf","tmpPath":"/s/meeting-1.notes.pdf.1-0.tmp",
            "imagesDir":"/s/meeting-1.notes/images","pages":[
            {"n":1,"widthPt":372,"heightPt":222,"viewport":[52,152,0.75],
             "content":[{"ref":"i:a","item":{"id":"a","type":"rectangle","known":true,"x":100,"y":200,"width":400,"height":200,"rotation":0,"style":{}}}]},
            {"n":2,"widthPt":595,"heightPt":842,"viewport":[0,0,0.75],"content":[]},
            {"n":3,"viewport":[0,0]}]}"""))!!
        assertEquals("meeting-1.notes.pdf", plan.path.name)
        assertEquals(2, plan.pages.size)
        assertEquals(Viewport(52.0, 152.0, 0.75), plan.pages[0].view)
        assertEquals(1, plan.pages[0].content.size)
        assertTrue(plan.pages[1].content.isEmpty())
        assertNull(PdfExport.Plan.from(JSONObject("{}")))
    }

    @Test fun theBarShowsTheStatusLine() {
        val s = ToolBarModel.State(NoteTool.PEN, collapsed = false, recording = false, status = "Exported · Notes PDF")
        assertTrue(ToolBarModel.items(s).contains(ToolBarModel.Item.Status("Exported · Notes PDF")))
        assertTrue(ToolBarModel.items(s.copy(collapsed = true)).none { it is ToolBarModel.Item.Status })
    }
}

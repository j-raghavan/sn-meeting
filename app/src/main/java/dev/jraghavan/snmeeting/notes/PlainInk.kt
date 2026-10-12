package dev.jraghavan.snmeeting.notes

import dev.jraghavan.snmeeting.notes.canvas.FirmwarePen

/**
 * F23-FR9: a tablet with no fast pen ink an app can use (the Viwoods AiPaper Mini: no Supernote
 * pen service, no iReader hand-write, and its own writing library crashes in an ordinary app,
 * device probe 2026-10-11). Nothing is drawn over the canvas, so the canvas draws every stroke
 * itself, and keeps a finished one on screen until the core has it ([NotesCanvasView.selfDrawn]).
 * Before this, such a tablet got a [SupernoteInk] with no service behind it, which did nothing.
 */
class PlainInk : PenInk {
    override var onReady: (() -> Unit)? = null
    override var pen: FirmwarePen = FirmwarePen.NEEDLE_BLACK
    override fun setup(reason: String): Boolean = false
    override fun clearAll() {}
    override fun setWritable(enable: Boolean) {}
    override fun setDisabledAreas(rects: List<PxRect>, where: String) {}
    override fun teardown() {}
    override fun fullRefresh() {}
    override fun systemGestures(enabled: Boolean) {}
}

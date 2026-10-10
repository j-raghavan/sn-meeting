package dev.jraghavan.snmeeting.notes

import android.app.Activity
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.Gravity
import android.view.View
import android.widget.FrameLayout
import android.widget.LinearLayout
import dev.jraghavan.snmeeting.Inkwell
import dev.jraghavan.snmeeting.InkDialog
import dev.jraghavan.snmeeting.NativeBridge
import dev.jraghavan.snmeeting.ScreenScope
import dev.jraghavan.snmeeting.model.CoreJson
import dev.jraghavan.snmeeting.model.CoreResult
import dev.jraghavan.snmeeting.model.str
import dev.jraghavan.snmeeting.notes.canvas.Badges
import dev.jraghavan.snmeeting.notes.canvas.DrawItem
import dev.jraghavan.snmeeting.notes.canvas.Handles
import dev.jraghavan.snmeeting.notes.canvas.TextLayout
import dev.jraghavan.snmeeting.record.LiveClock
import org.json.JSONObject
import java.io.File

/**
 * DEVICE-UNVERIFIED (wiring of views; the rules are in the pure classes it uses). Layout B's
 * Notes tab (F17-FR1): the infinite canvas with its zoom pill and minimap. Strokes go to the
 * core as they end; the core keeps them in memory and this page flushes them on a short
 * deferred timer ([SAVE_MS], inkread's 1 500 ms) and when it closes, with the viewport.
 *
 * F17-FR12: the page never touches the recording. Leaving it, the screen sleeping or another
 * app in front only saves the ink and gives the pen back; the recording service and the
 * near-live lane carry on. Strokes written once the recording stops are untimed.
 */
class NotesPage(
    private val a: Activity,
    private val ui: Inkwell,
    private val scope: ScreenScope,
    private val id: String,
    private val host: Host,
) {
    interface Host {
        /** The recording under way is this meeting's: its start (elapsed ms), else null. */
        fun recordingSince(): Long?
        /** F17-FR2: the bar's side and state, from settings.json. */
        fun prefs(): NotesPrefs
        fun savePrefs(patch: String)
        /** The timer slot: "0:12:41" while recording, the position when not. */
        fun timer(): String
        /** F16-FR5: "≈ $0.08 so far" while recording near-live, else null. */
        fun cost(): String?
        fun playing(): Boolean
        fun canPlay(): Boolean
        fun togglePlay()
        fun mark()
        fun stop()
        /** F17-FR7: Listen found ink written at a moment: play from [seekMs]; [lineN] is the
         *  transcript line spoken then (or null). */
        fun listenAt(seekMs: Long, lineN: Int?)
        /** The summarise provider ("anthropic"), for the notes notice; null without a key. */
        fun summariseProvider(): String?
    }

    private val handler = Handler(Looper.getMainLooper())
    private val save = Runnable { scope.core({ NativeBridge.nativeInkSave(id) }) { json -> reportError(json, "Couldn't save the notes") } }
    private var opened = false
    /** The Notes tab is in front: only then is the pen claimed for firmware ink. */
    private var front = true

    var tool: NoteTool = NoteTool.PEN
        private set
    /** F21-FR2: the last shape used shows on Shapes ▸. */
    private var lastShape = NoteTool.RECTANGLE
    /** F17-FR11 / R14: the strip's and the timer's redraws wait while the pen writes. */
    val quiet = PenQuiet()
    /** F17-FR14: dialogs wait while the pen is in use (a dialog takes the pen's claim). */
    val dialogs = DialogGate({ SystemClock.uptimeMillis() })
    private val dialogTick = Runnable { dialogs.tick() }
    /** F21-FR11: the canvas's image copies, decoded for drawing. */
    val images = dev.jraghavan.snmeeting.notes.canvas.ImageCache()


    val canvas: NotesCanvasView = NotesCanvasView(a, object : NotesCanvasView.Host {
        override val tool get() = this@NotesPage.tool
        override val recording get() = host.recordingSince() != null
        override val penWidthPx get() = PEN_WIDTH_PX
        override fun touched() { hideHints(); panels.close() }

        override fun pen(down: Boolean) {
            if (down) quiet.down() else quiet.up()
            dialogs.pen(down)
            handler.removeCallbacks(dialogTick)
            if (!down) handler.postDelayed(dialogTick, DialogGate.IDLE_MS)
        }

        override fun strokeTimes(): StrokeTimes {
            val since = host.recordingSince() ?: return StrokeTimes.UNTIMED
            return StrokeTimes.recording(SystemClock.uptimeMillis(), SystemClock.elapsedRealtime(), since) { LiveClock.current }
        }

        override fun commitStroke(tool: NoteTool, widthWorld: Float, xypt: FloatArray) = changed(
            if (tool == NoteTool.PEN) InkBake.Event.PEN_STROKE else InkBake.Event.HIGHLIGHTER_STROKE,
        ) {
            // F21-FR7: the Pen draws in the default style (pressure, size, colour).
            if (tool == NoteTool.PEN) NativeBridge.nativeInkStrokeStyled(id, tool.core, canvas.inkView.defaultStyle.toJson(), xypt)
            else NativeBridge.nativeInkStroke(id, LAYER, tool.core, widthWorld, xypt)
        }

        override fun erase(xy: FloatArray, radiusWorld: Float) = changed(InkBake.Event.ERASE) { NativeBridge.nativeInkErase(id, LAYER, xy, radiusWorld) }
        // F17 / F21-FR4: the lasso takes strokes and enclosed items; the selection is the core's.
        override fun lasso(xy: FloatArray) = select(polygonOp(xy))

        override fun moveSelection(dx: Float, dy: Float) =
            edit("""{"op":"move","dx":$dx,"dy":$dy}""", keepSelection = true)

        override fun selectAt(x: Float, y: Float, radiusWorld: Float) {
            if (x.isNaN()) return clearSelection()
            // F21-FR9: a pen tap on a selected text box or note edits it again.
            val sole = canvas.inkView.selection.singleOrNull()
            val text = textItemAt(x.toDouble(), y.toDouble())
            if (text != null && text.ref == sole) return editText(text)
            // F21-FR10: a pen tap on a table's cell targets it and opens the keyboard for it.
            if (cellTap(x.toDouble(), y.toDouble())) return
            target = null
            select("""{"op":"select","at":[$x,$y],"r":$radiusWorld}""")
        }

        override fun openLink(ref: String) = openLinkOf(ref)

        override fun createTable(x0: Float, y0: Float, x1: Float, y1: Float) =
            edit("""{"op":"create","type":"table","from":[$x0,$y0],"to":[$x1,$y1]${recMs()}}""")

        override fun tableTap(x: Float, y: Float) { cellTap(x.toDouble(), y.toDouble()) }

        override fun rowDragged(ref: String, row: Int, x: Float, y: Float) {
            val item = canvas.inkView.items.firstOrNull { it.ref == ref } ?: return
            val h = dev.jraghavan.snmeeting.notes.canvas.TableCells.rowHeightAt(item, row, x.toDouble(), y.toDouble())
            edit("""{"op":"table.resizeRow","ref":${CoreJson.quote(ref)},"row":$row,"height":$h}""", keepSelection = true)
        }

        override fun place(tool: NoteTool, x: Float, y: Float) {
            val hit = textItemAt(x.toDouble(), y.toDouble())
            if (hit != null) editText(hit) else newText(tool, x.toDouble(), y.toDouble())
        }

        override fun selectRect(x0: Float, y0: Float, x1: Float, y1: Float) =
            select("""{"op":"select","rect":[$x0,$y0,$x1,$y1]}""")

        override fun createShape(tool: NoteTool, x0: Float, y0: Float, x1: Float, y1: Float) =
            edit("""{"op":"create","type":"${tool.name.lowercase()}","from":[$x0,$y0],"to":[$x1,$y1]${recMs()}}""")

        override fun handleDragged(ref: String, h: Handles.Handle, x: Float, y: Float) = edit(handleOp(ref, h, x, y), keepSelection = true)
        override fun listen(x: Float, y: Float, radiusWorld: Float) = scope.core({ NativeBridge.nativeInkHit(id, LAYER, x, y, radiusWorld) }) { json ->
            val hit = CoreJson.obj(json, ListenHit::from).getOrNull() ?: return@core
            val seek = hit.seekMs ?: return@core // an untimed stroke, or no stroke: nothing happens
            hit.lineN?.let { n -> showStrip(strip.state.copy(highlight = n)) }
            host.listenAt(seek, hit.lineN)
        }
        override fun viewportChanged(v: Viewport) = fetch(InkBake.Event.VIEWPORT_SETTLED)
        override fun allEntries(bounds: WorldRect, done: (List<DrawEntry>) -> Unit) = scope.core({
            NativeBridge.nativeCanvasForDraw(id, bounds.left.toFloat(), bounds.top.toFloat(), bounds.right.toFloat(), bounds.bottom.toFloat())
        }) { json -> done(CoreJson.obj(json, InkView::from).getOrNull()?.entries.orEmpty()) }
    })

    /** F17-FR14: the zoom pill's controls the hints point at. */
    private val pillAnchors = HashMap<String, View>()

    private val zoomPill = LinearLayout(a).apply {
        orientation = LinearLayout.HORIZONTAL
        background = ui.outline(ui.dim(30))
        setPadding(ui.dim(8), ui.dim(4), ui.dim(8), ui.dim(4))
        val item = { label: String, act: () -> Unit ->
            ui.eyebrow(label).apply {
                setPadding(ui.dim(12), ui.dim(10), ui.dim(12), ui.dim(10))
                isClickable = true; setOnClickListener { hideHints(); act() }
            }.also { addView(it) }
        }
        pillAnchors["zoom"] = item("FIT") { canvas.fit() }
        Viewport.PRESETS.forEach { p -> item("$p%") { canvas.zoomPercent(p) } }
        pillAnchors["map"] = item("MAP") { canvas.minimapOpen = !canvas.minimapOpen } // F17-FR13: on demand
    }

    /** The canvas with the zoom pill over its top corner. */
    val canvasFrame = FrameLayout(a).apply {
        addView(canvas, FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT))
        addView(zoomPill, FrameLayout.LayoutParams(FrameLayout.LayoutParams.WRAP_CONTENT, FrameLayout.LayoutParams.WRAP_CONTENT,
            Gravity.TOP or Gravity.START).apply { topMargin = ui.dim(12); marginStart = ui.dim(12) })
    }

    /** F17-FR6: the 3-line transcript strip; tapping it opens every line over the lower half. */
    val strip: TranscriptStrip = TranscriptStrip(a, ui) { showStrip(strip.state.toggled()) }

    /** The canvas column: the canvas over the strip. */
    val column = LinearLayout(a).apply {
        orientation = LinearLayout.VERTICAL
        addView(canvasFrame, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, 0, 1f))
        addView(strip.view, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT))
    }

    /** F17-FR3: the meeting's marks (recording-clock ms), each a ◆ beside the stroke before it. */
    private var marks: List<Long> = emptyList()

    fun setMarks(atMs: List<Long>) {
        if (atMs == marks) return
        marks = atMs
        placeMarks()
    }

    private fun placeMarks() {
        val g = MarkGlyphs.place(canvas.inkView.strokes, marks)
        if (g != canvas.markGlyphs) canvas.markGlyphs = g
    }

    /** A meeting with near-live lines has a strip; a Standard one's transcript is on Minutes. */
    var liveAvailable = false
        set(v) { field = v; showStrip(strip.state) }

    /** New lines or a new status from the core (about twice a minute): the strip redraws only if they change it. */
    fun showLive(t: dev.jraghavan.snmeeting.model.LiveTranscript) = quiet.redraw("strip") { showStrip(strip.state.copy(transcript = t)) }

    /** Draw [next]; opened, the strip takes the lower half of the column, its ink above still shown. */
    fun showStrip(next: StripModel) {
        val wasOpen = strip.state.open
        strip.show(next.copy(focus = prefs.focus || !liveAvailable))
        if (next.open != wasOpen) {
            val h = if (next.open) column.height / 2 else LinearLayout.LayoutParams.WRAP_CONTENT
            strip.view.layoutParams = LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, h)
        }
    }

    /** F17-FR2: the docked bar, on the canvas's right (or left) edge, never over it. */
    val bar: DockedToolBar = DockedToolBar(a, ui, object : DockedToolBar.Host {
        override fun onTool(t: NoteTool) = pickTool(t)
        override fun onHelp() = if (hints.showing) hints.hide() else hints.show()
        override fun onUndo() = changed(InkBake.Event.UNDO_REDO) { NativeBridge.nativeInkUndo(id, LAYER) }
        override fun onRedo() = changed(InkBake.Event.UNDO_REDO) { NativeBridge.nativeInkRedo(id, LAYER) }
        override fun onCollapse(collapsed: Boolean) {
            panels.close()
            host.savePrefs(NotesPatch.collapsed(collapsed))
            prefs = prefs.copy(collapsed = collapsed)
            layout()
        }
        override fun onPanel(panel: ToolBarModel.Panel, anchor: View) = togglePanel(panel, anchor)
        override fun onAction(act: ToolBarModel.Act, anchor: View) = selectionAction(act, anchor)
        override fun onMark() = host.mark()
        override fun onStop() = host.stop()
        override fun onPlay() { host.togglePlay(); refreshBar() }
    })

    /** F21-FR2: a tool picked on the bar or in a flyout. Closing a panel bakes with the new
     *  tool's ink state, so the change costs one redraw. */
    private fun pickTool(t: NoteTool) {
        hideHints()
        // F21-FR3: the Eraser tapped again while it is the tool offers Clear canvas beside it.
        if (t == NoteTool.ERASER && tool == NoteTool.ERASER && panels.open != ToolBarModel.Panel.ERASER &&
            canvas.inkView.bounds != null) {
            val anchor = bar.anchors["eraser"] ?: return
            panels.show(ToolBarModel.Panel.ERASER, anchor, panels.rows(listOf(MoreMenu.Entry.CLEAR.label)) { askClear() })
            refreshBar()
            return
        }
        tool = t
        if (t.isShape) lastShape = t
        clearSelection()
        if (panels.view != null) panels.close() else canvas.toolChanged(t)
        refreshBar()
    }

    /** F21-FR2: a flyout or panel button: the same one again closes it. */
    private fun togglePanel(panel: ToolBarModel.Panel, anchor: View) {
        hideHints()
        if (panels.open == panel) { panels.close(); refreshBar(); return }
        if (panel == ToolBarModel.Panel.STYLE) styleAnchor = anchor
        val content = when (panel) {
            ToolBarModel.Panel.SHAPES -> panels.icons(NoteTool.SHAPES.map { Triple(bar.iconFor(it), it.label, it == tool) }) { i ->
                pickTool(NoteTool.SHAPES[i])
            }
            ToolBarModel.Panel.INSERT -> panels.icons(NoteTool.INSERTS.map { Triple(bar.iconFor(it), it.label, it == tool) } +
                Triple(dev.jraghavan.snmeeting.R.drawable.ic_cv_tool_image, "Image", false)) { i ->
                if (i < NoteTool.INSERTS.size) pickTool(NoteTool.INSERTS[i]) else pickImage()
            }
            ToolBarModel.Panel.MORE -> moreMenu()
            ToolBarModel.Panel.STYLE -> stylePanel()
            ToolBarModel.Panel.TABLE -> tableMenu() ?: return
            else -> return
        }
        panels.show(panel, anchor, content)
        refreshBar()
    }

    /** F21-FR6: the style panel for the selection (or, with none, the default). */
    private val stylePanelView = dev.jraghavan.snmeeting.notes.canvas.StylePanel(a, ui)
    private var styleAnchor: View? = null

    private fun stylePanel(): View {
        val v = canvas.inkView
        val picked = v.selection.mapNotNull { r -> v.entries.firstOrNull { it.ref == r } }
        val kinds = picked.map { e -> if (e is DrawEntry.Thing) e.item.type else "stroke" }.toSet()
        val shown = picked.firstNotNullOfOrNull { e ->
            when (e) {
                is DrawEntry.Thing -> e.item.style
                is DrawEntry.Ink -> e.stroke.style
            }
        } ?: v.defaultStyle
        return stylePanelView.build(shown, kinds) { prop, value ->
            val q = CoreJson.quote(value)
            if (canvas.inkView.selection.isEmpty()) {
                edit("""{"op":"setDefaultStyle","property":"$prop","value":$q}""")
            } else {
                edit("""{"op":"restyle","property":"$prop","value":$q}""", keepSelection = true)
            }
        }
    }

    /** F21-FR6: after a pick the panel shows the new style (one redraw of the panel). */
    private fun refreshStylePanel() {
        val anchor = styleAnchor ?: return
        if (panels.open == ToolBarModel.Panel.STYLE) panels.show(ToolBarModel.Panel.STYLE, anchor, stylePanel())
    }

    /** F21-FR12: ⋮, with what applies now. */
    private fun moreMenu(): View {
        val ink = canvas.inkView
        val entries = MoreMenu.items(MoreMenu.State(selection = selectedCount() > 0, focus = prefs.focus, tableCell = tableTargeted(),
            canvases = true, export = true,
            hasContent = ink.bounds != null, canRestore = ink.canRestore))
        return panels.rows(entries.map { it.label }) { i -> more(entries[i]) }
    }

    private fun more(e: MoreMenu.Entry) {
        panels.close()
        when (e) {
            MoreMenu.Entry.ZOOM_FIT -> canvas.fit()
            MoreMenu.Entry.ZOOM_100 -> canvas.zoomPercent(100)
            MoreMenu.Entry.FOCUS_ON, MoreMenu.Entry.FOCUS_OFF -> {
                prefs = prefs.copy(focus = !prefs.focus)
                host.savePrefs(NotesPatch.focus(prefs.focus))
                showStrip(strip.state) // F17-AC8: hidden, or back with the newest lines
            }
            MoreMenu.Entry.REFRESH -> canvas.fullRefresh()
            MoreMenu.Entry.CLEAR -> askClear()
            MoreMenu.Entry.TABLE -> bar.anchors["more"]?.let { m -> tableMenu()?.let { panels.show(ToolBarModel.Panel.TABLE, m, it) } }
            MoreMenu.Entry.FRONT -> edit("""{"op":"front"}""", keepSelection = true)
            MoreMenu.Entry.BACK -> edit("""{"op":"back"}""", keepSelection = true)
            MoreMenu.Entry.RESTORE -> edit("""{"op":"restoreCleared"}""")
            MoreMenu.Entry.NEW_CANVAS -> newCanvas()
            MoreMenu.Entry.EXPORT -> exportPdf()
            MoreMenu.Entry.CANVASES -> canvasList()
            else -> Unit
        }
        refreshBar()
    }

    /**
     * F21-FR3: "Clear this canvas?" (InkDialog), asked for by the user: it opens only with no
     * stroke in progress (a request with the pen down is dropped, EC32). Closing it re-claims
     * the pen and bakes once.
     */
    private fun askClear() {
        panels.close()
        dialogs.explicit {
            InkDialog.confirm(a, CLEAR_TITLE, CLEAR_BODY, CLEAR_YES, onClose = {
                claim("focus")
                canvas.bake(InkBake.Event.PANEL_CLOSED)
            }) { edit("""{"op":"clear"}""") }
        }
        refreshBar()
    }

    /** F21-FR9: the keyboard editor over the canvas. */
    private val textEditor by lazy { dev.jraghavan.snmeeting.notes.canvas.TextEditor(a, canvasFrame) }

    /** The topmost text box or note under a world point (shell geometry, rotation included). */
    private fun textItemAt(x: Double, y: Double): DrawItem? = canvas.inkView.items.lastOrNull { it ->
        (it.type == "text" || it.type == "note") && it.toLocal(x, y).let { l ->
            l.x in it.x..(it.x + it.width) && l.y in it.y..(it.y + it.height)
        }
    }

    private fun openEditor(t: dev.jraghavan.snmeeting.notes.canvas.TextEditor.Target, wx: Double, wy: Double, ww: Double,
        font: Double, initial: String, done: (String) -> Unit) {
        val v = canvas.viewport
        canvas.overlayOff(true)
        canvas.editing = t.itemId?.let { it to t.cell }
        textEditor.show(t, v.screenX(wx).toInt(), v.screenY(wy).toInt(), (ww * v.zoom).toInt(), (font * v.zoom).toFloat(), initial) { text ->
            canvas.editing = null
            canvas.overlayOff(false)
            done(text)
        }
    }

    /** F21-FR9: a new text box (top-left at the tap) or note (centred on it); placing it and
     *  its text is one step, made when Done or Back commits; an empty one is discarded. */
    private fun newText(tool: NoteTool, x: Double, y: Double) {
        val note = tool == NoteTool.NOTE
        val style = canvas.inkView.defaultStyle
        val w = if (note) NOTE_SIDE else TEXT_WIDTH
        val (left, top) = if (note) (x - w / 2) to (y - w / 2) else x to y
        openEditor(dev.jraghavan.snmeeting.notes.canvas.TextEditor.Target(null, null), left, top, w, style.size.fontSize, "") { text ->
            if (text.isBlank()) return@openEditor
            val h = TextLayout.height(text, w, style.size.fontSize)
            edit("""{"op":"create","type":"${if (note) "note" else "text"}","at":[$x,$y],"text":${CoreJson.quote(text)},"height":$h${recMs()}}""")
        }
    }

    /** F21-FR9: edit a text box's or note's text again: one step per session. */
    private fun editText(item: DrawItem) {
        openEditor(dev.jraghavan.snmeeting.notes.canvas.TextEditor.Target(item.id, null), item.x, item.y, item.width,
            item.style.size.fontSize, item.text.orEmpty()) { text ->
            if (text == item.text) return@openEditor
            val h = TextLayout.height(text, item.width, item.style.size.fontSize)
            edit("""{"op":"setText","ref":${CoreJson.quote(item.ref)},"text":${CoreJson.quote(text)},"height":$h${recMs()}}""", keepSelection = true)
        }
    }

    /** F21-FR10: the target cell (table ref, row, col), marked with corner brackets. */
    private var target: Triple<String, Int, Int>? = null
        set(v) {
            field = v
            canvas.targetCell = v?.let { (r, row, col) ->
                canvas.inkView.items.firstOrNull { it.ref == r }?.let { dev.jraghavan.snmeeting.notes.canvas.TableCells.rect(it, row, col) }
            }
        }

    /** A tap on a table cell: it becomes the target (its table selected) and is typed into. */
    private fun cellTap(x: Double, y: Double): Boolean {
        val table = canvas.inkView.items.lastOrNull { it.table != null && dev.jraghavan.snmeeting.notes.canvas.TableCells.at(it, x, y) != null }
            ?: return false
        val (row, col) = dev.jraghavan.snmeeting.notes.canvas.TableCells.at(table, x, y) ?: return false
        target = Triple(table.ref, row, col)
        if (canvas.inkView.selection != listOf(table.ref)) select("""{"op":"select","refs":[${CoreJson.quote(table.ref)}]}""")
        editCell(table, row, col)
        return true
    }

    /** F21-FR10: type into a cell; committed as F21-FR9's text, one step; the row grows. */
    private fun editCell(item: DrawItem, row: Int, col: Int) {
        val t = item.table ?: return
        val box = dev.jraghavan.snmeeting.notes.canvas.TableCells.rect(item, row, col) ?: return
        val font = item.style.size.fontSize * TextLayout.TABLE_FONT_SCALE
        val initial = t.cells[row * t.cols + col]
        openEditor(dev.jraghavan.snmeeting.notes.canvas.TextEditor.Target(item.id, row * t.cols + col), box.left, box.top,
            box.right - box.left, font, initial) { text ->
            if (text == initial) return@openEditor
            val h = if (text.isEmpty()) 0.0 else TextLayout.height(text, box.right - box.left, font)
            edit("""{"op":"table.setCell","ref":${CoreJson.quote(item.ref)},"row":$row,"col":$col,"text":${CoreJson.quote(text)},"height":$h${recMs()}}""",
                keepSelection = true)
        }
    }

    /** F21-FR10: the Table actions on the target cell; Remove is off at one row or column, Add
     *  at the limit (12 × 8). */
    private fun tableMenu(): View? {
        val (ref, row, col) = target ?: return null
        val t = canvas.inkView.items.firstOrNull { it.ref == ref }?.table ?: return null
        val labels = listOf("Add row", "Add column", "Remove this row", "Remove this column")
        val on = listOf(t.rows < 12, t.cols < 8, t.rows > 1, t.cols > 1)
        val ops = listOf("table.addRow" to "row", "table.addColumn" to "col", "table.removeRow" to "row", "table.removeColumn" to "col")
        return panels.rows(labels, { on[it] }) { i ->
            panels.close()
            val (op, k) = ops[i]
            val v = if (k == "row") row else col
            target = null
            edit("""{"op":"$op","ref":${CoreJson.quote(ref)},"$k":$v}""", keepSelection = true)
        }
    }

    /**
     * F21-FR11: Insert ▸ Image: an Inkwell list of the images in shared storage, newest first
     * (asked for by the user, so it opens with the pen up). The core checks and copies the pick
     * and places it in the view; closing the list re-claims the pen and bakes.
     */
    private fun pickImage() {
        panels.close()
        scope.core({
            dev.jraghavan.snmeeting.notes.canvas.ImageFiles.list(android.os.Environment.getExternalStorageDirectory()).joinToString("\n") { it.path }
        }) { paths ->
            val files = paths.lines().filter { it.isNotEmpty() }.map(::File)
            dialogs.explicit {
                if (files.isEmpty()) {
                    InkDialog.error(a, "No images", "There are no PNG, JPG or WebP images in Pictures, Download, INBOX, SCREENSHOT or EXPORT.")
                    return@explicit
                }
                android.app.AlertDialog.Builder(a, dev.jraghavan.snmeeting.R.style.InkDialog).setTitle("Insert an image")
                    .setItems(files.map { it.name }.toTypedArray()) { _, i -> insertImage(files[i]) }
                    .setNegativeButton("CANCEL", null)
                    .setOnDismissListener { claim("focus"); canvas.bake(InkBake.Event.PANEL_CLOSED) }
                    .show()
            }
        }
    }

    private fun insertImage(f: java.io.File) {
        val v = canvas.viewport.visible(canvas.width.toDouble(), canvas.height.toDouble())
        val view = """{"rect":[${v.left},${v.top},${v.right},${v.bottom}]${recMs()}}"""
        scope.core({
            val (w, h) = dev.jraghavan.snmeeting.notes.canvas.ImageCache.sizeOf(f) ?: (0 to 0)
            NativeBridge.nativeCanvasImportImage(id, f.path, w, h, view)
        }) { json ->
            if (!reportError(json, "Couldn't insert that image")) {
                images.retry()
                fetch(InkBake.Event.EDIT)
                scheduleSave()
            }
        }
    }

    /**
     * F21-FR13: Link ⇢ meeting: the recent meetings, newest first, filtered by a search over
     * their titles; a pick links every selected item and stroke (one step). Remove link clears
     * it. Links are never sent to a provider.
     */
    private fun linkPicker() {
        val linked = canvas.inkView.selection.any { r -> canvas.inkView.entries.firstOrNull { it.ref == r }?.let(Badges::linked) == true }
        scope.core({ NativeBridge.nativeListRecent(200) }) { json ->
            val all = CoreJson.list(json, dev.jraghavan.snmeeting.model.MeetingSummary::from).getOrNull().orEmpty().filter { it.id != id }
            dialogs.explicit {
                val search = ui.field("").apply { hint = "Search titles" }
                val list = android.widget.ListView(a)
                var shown = all
                fun label(m: dev.jraghavan.snmeeting.model.MeetingSummary) =
                    (m.title?.takeIf { it.isNotBlank() } ?: "Untitled") + "  ·  " + m.startedAt.take(16).replace('T', ' ')
                fun refill() {
                    val q = search.text.toString().trim().lowercase()
                    shown = if (q.isEmpty()) all else all.filter { (it.title ?: "").lowercase().contains(q) }
                    list.adapter = android.widget.ArrayAdapter(a, android.R.layout.simple_list_item_1, shown.map(::label))
                }
                refill()
                search.addTextChangedListener(object : android.text.TextWatcher {
                    override fun beforeTextChanged(s: CharSequence?, st: Int, c: Int, af: Int) {}
                    override fun onTextChanged(s: CharSequence?, st: Int, b: Int, c: Int) {}
                    override fun afterTextChanged(s: android.text.Editable?) = refill()
                })
                val box = LinearLayout(a).apply {
                    orientation = LinearLayout.VERTICAL
                    setPadding(ui.dim(24), ui.dim(8), ui.dim(24), 0)
                    addView(search)
                    addView(list, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, ui.dp(420)))
                }
                val b = android.app.AlertDialog.Builder(a, dev.jraghavan.snmeeting.R.style.InkDialog).setTitle("Link to a meeting")
                    .setView(box)
                    .setNegativeButton("CANCEL", null)
                    .setOnDismissListener { claim("focus"); canvas.bake(InkBake.Event.PANEL_CLOSED) }
                if (linked) b.setNeutralButton("REMOVE LINK") { _, _ -> edit("""{"op":"unlink"}""", keepSelection = true) }
                val d = b.show()
                list.setOnItemClickListener { _, _, i, _ ->
                    d.dismiss()
                    edit("""{"op":"link","target":${CoreJson.quote(shown[i].id)}}""", keepSelection = true)
                }
            }
        }
    }

    /** F21-FR13: a badge opens the linked meeting on its Minutes tab; Back returns here. */
    private fun openLinkOf(ref: String) {
        scope.core({ NativeBridge.nativeCanvasLinkTarget(id, ref) }) { json ->
            val o = CoreJson.obj(json) { it }.getOrNull() ?: return@core
            val target = o.str("meetingId") ?: return@core
            if (o.optBoolean("missing")) {
                dialogs.explicit { InkDialog.info(a, "That meeting was deleted", "The link stays until you remove it.") }
                return@core
            }
            a.startActivity(android.content.Intent(a, dev.jraghavan.snmeeting.MeetingActivity::class.java)
                .putExtra(dev.jraghavan.snmeeting.MeetingActivity.EXTRA_ID, target)
                .putExtra(dev.jraghavan.snmeeting.MeetingActivity.EXTRA_TAB, dev.jraghavan.snmeeting.MeetingActivity.MINUTES))
            a.overridePendingTransition(0, 0)
        }
    }

    /** F21-FR14: the bar's status line, shown for a while after an export. */
    private var status: String? = null
    private val clearStatus = Runnable { status = null; refreshBar() }

    private fun showStatus(text: String) {
        status = text
        refreshBar()
        handler.removeCallbacks(clearStatus)
        handler.postDelayed(clearStatus, STATUS_MS)
    }

    /** F21-FR14: ⋮ → Export to PDF: the core plans the pages, the shell draws them off the UI
     *  thread and writes `meeting-….notes.pdf` beside the sidecar (tmp, fsync, rename). */
    private fun exportPdf() {
        showStatus("Exporting…")
        scope.core({ NativeBridge.nativeInkSave(id); NativeBridge.nativeCanvasPdfPlan(id) }) { json ->
            val plan = CoreJson.obj(json, dev.jraghavan.snmeeting.notes.canvas.PdfExport.Plan::from)
            val p = (plan as? CoreResult.Ok)?.value
            if (p == null) {
                showStatus("Export failed")
                reportError(json, "Couldn't export the notes")
                return@core
            }
            val deliver = scope.deliver<String?> { err -> showStatus(if (err == null) "Exported · Notes PDF" else "Export failed: $err") }
            Thread({
                val err = dev.jraghavan.snmeeting.notes.canvas.PdfExport.write(p)
                a.runOnUiThread { deliver(err) }
            }, "notes-pdf").start()
        }
    }

    /** F21-FR12: New canvas saves the one shown and shows a new, empty one (not an undo step). */
    private fun newCanvas() {
        clearSelection()
        target = null
        openWith({
            val r = NativeBridge.nativeCanvasNew(id)
            val n = CoreJson.obj(r) { it.optInt("n", 0) }.getOrNull()
            if (n == null || n == 0) r else NativeBridge.nativeCanvasShow(id, n)
        }, null, hintsIfEmpty = false)
    }

    /** F21-FR12: ⋮ → Canvases…: one row per canvas ("Canvas 2 · 12:41 · 34 strokes, 5 items");
     *  a tap shows it. An empty canvas that isn't shown can be deleted. */
    private fun canvasList() {
        scope.core({ NativeBridge.nativeCanvasList(id) }) { json ->
            val o = CoreJson.obj(json) { it }.getOrNull() ?: return@core
            val rows = CanvasRows.of(o)
            dialogs.explicit {
                val labels = rows.map { it.label } + rows.filter { it.deletable }.map { "Delete empty Canvas ${it.n}" }
                android.app.AlertDialog.Builder(a, dev.jraghavan.snmeeting.R.style.InkDialog).setTitle("Canvases")
                    .setItems(labels.toTypedArray()) { _, i ->
                        if (i < rows.size) {
                            if (!rows[i].current) { clearSelection(); target = null; open(canvasN = rows[i].n) }
                        } else {
                            val n = rows.filter { it.deletable }[i - rows.size].n
                            scope.core({ NativeBridge.nativeCanvasEdit(id, """{"op":"deleteCanvas","n":$n}""") }) { r ->
                                reportError(r, "Couldn't delete that canvas")
                            }
                        }
                    }
                    .setNegativeButton("CLOSE", null)
                    .setOnDismissListener { claim("focus"); canvas.bake(InkBake.Event.PANEL_CLOSED) }
                    .show()
            }
        }
    }

    /** F21-FR15 / OQ36: `,"recMs":t` for something made now while recording, else nothing. */
    private fun recMs(): String {
        val since = host.recordingSince() ?: return ""
        val t = StrokeTimes.recording(SystemClock.uptimeMillis(), SystemClock.elapsedRealtime(), since) { LiveClock.current }
            .recMs(SystemClock.uptimeMillis()) ?: return ""
        return ""","recMs":$t"""
    }

    /** F21: one gesture on the canvas through the core (one undo step), then what is in view
     *  is fetched again and baked, and the deferred save armed. */
    private fun edit(op: String, keepSelection: Boolean = false) {
        if (!keepSelection) clearSelection()
        changed(InkBake.Event.EDIT) { NativeBridge.nativeCanvasEdit(id, op) }
    }

    /** F21-FR2: the selection section's actions. */
    private fun selectionAction(act: ToolBarModel.Act, anchor: View) {
        hideHints()
        when (act) {
            ToolBarModel.Act.DELETE -> edit("""{"op":"delete"}""")
            ToolBarModel.Act.DUPLICATE -> edit("""{"op":"duplicate"}""", keepSelection = true)
            ToolBarModel.Act.DONE -> clearSelection()
            ToolBarModel.Act.STYLE -> togglePanel(ToolBarModel.Panel.STYLE, anchor)
            ToolBarModel.Act.TABLE -> togglePanel(ToolBarModel.Panel.TABLE, anchor)
            ToolBarModel.Act.LINK -> linkPicker()
            ToolBarModel.Act.GROUP -> edit("""{"op":"group"}""", keepSelection = true)
            ToolBarModel.Act.UNGROUP -> edit("""{"op":"ungroup"}""", keepSelection = true)
            ToolBarModel.Act.FRONT -> edit("""{"op":"front"}""", keepSelection = true)
            ToolBarModel.Act.BACK -> edit("""{"op":"back"}""", keepSelection = true)
            else -> Unit
        }
        refreshBar()
    }

    private fun selectedCount(): Int = canvas.inkView.selection.size

    /** F21-FR10: a cell of the one selected table is the target. */
    private fun tableTargeted(): Boolean = target?.first?.let { canvas.inkView.selection == listOf(it) } ?: false

    /** F21-FR4: the selection the bar acts on: its size, and whether it is one group. */
    private fun selectionState(): ToolBarModel.Selection? {
        val v = canvas.inkView
        if (v.selection.isEmpty()) return null
        val groups = v.selection.map { r -> v.entries.firstOrNull { it.ref == r }?.let(::groupOf) }
        val grouped = groups.isNotEmpty() && groups.all { it != null && it == groups[0] }
        return ToolBarModel.Selection(v.selection.size, grouped = grouped, tableCell = tableTargeted())
    }

    private fun groupOf(e: DrawEntry): String? = when (e) {
        is DrawEntry.Ink -> e.stroke.groupId
        is DrawEntry.Thing -> e.item.groupId
    }

    /** F21-FR4: a selection op (not an undo step): one fetch, one redraw of canvas and bar. */
    private fun select(op: String) = scope.core({ NativeBridge.nativeCanvasEdit(id, op) }) { json ->
        if (!reportError(json, "Couldn't select that")) fetch(null)
    }

    /**
     * F21-FR5: the edit a handle drag makes. A text box or note resized keeps its text size, so
     * its new height is measured here at the new width; a table's cells are measured at its new
     * column width (the core can't measure text).
     */
    private fun handleOp(ref: String, h: Handles.Handle, x: Float, y: Float): String {
        val q = CoreJson.quote(ref)
        return when (h.kind) {
            Handles.Kind.ROTATE -> """{"op":"rotate","ref":$q,"x":$x,"y":$y}"""
            Handles.Kind.END -> """{"op":"endpoint","ref":$q,"which":"${h.id}","x":$x,"y":$y}"""
            Handles.Kind.CORNER -> {
                val item = canvas.inkView.items.firstOrNull { it.ref == ref }
                val extra = item?.let { measure(it, Handles.resizedWidth(it, h.id, x.toDouble(), y.toDouble())) }.orEmpty()
                """{"op":"resize","ref":$q,"corner":"${h.id}","x":$x,"y":$y$extra}"""
            }
        }
    }

    /** What the core needs measured for [item] at [width]: `,"fit":h` or `,"cellHeights":[…]`. */
    private fun measure(item: DrawItem, width: Double): String {
        val font = item.style.size.fontSize
        val table = item.table
        return when {
            item.type == "text" || item.type == "note" -> ""","fit":${TextLayout.height(item.text.orEmpty(), width, font)}"""
            table != null -> {
                val col = width / table.cols
                val hs = table.cells.map { if (it.isEmpty()) 0.0 else TextLayout.height(it, col, font * TextLayout.TABLE_FONT_SCALE) }
                ""","cellHeights":[${hs.joinToString(",")}]"""
            }
            else -> ""
        }
    }

    private fun polygonOp(xy: FloatArray) = """{"op":"select","polygon":[${xy.joinToString(",")}]}"""

    private fun clearSelection() {
        if (canvas.inkView.selection.isEmpty()) return
        select("""{"op":"select","none":true}""")
    }

    private var prefs = host.prefs()

    /** The tab's content: the bar beside the canvas column. */
    private val row = LinearLayout(a).apply { orientation = LinearLayout.HORIZONTAL }

    /** F17-FR14: the hand-drawn hints, over everything on the tab; they never take a touch. */
    private val hints: HintsOverlay = HintsOverlay(a, ::hintTips, ::inkCovers) { prefs.dock == NotesPrefs.Dock.LEFT }

    /** F21-FR2: where panels open beside the bar (empty, it takes no touch). */
    private val panelLayer = FrameLayout(a)

    /** The tab's content: the bar beside the canvas column, its panels, the hints over both. */
    val view = FrameLayout(a).apply {
        addView(row, FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT))
        addView(panelLayer, FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT))
        addView(hints, FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT))
    }

    /** F21-FR2: one panel at a time beside the bar; firmware ink is off while one is open. */
    private val panels: SidePanels = SidePanels(a, ui, panelLayer, { prefs.dock == NotesPrefs.Dock.LEFT },
        { if (prefs.collapsed) bar.collapsedPx else bar.expandedPx }) { open ->
        canvas.panelOpen(open)
    }

    fun hideHints() = hints.hide()

    /** Each control's arrow tip in the hints' pixels: the side of a bar control facing the
     *  canvas, the bottom of a zoom-pill control, the top of the strip. */
    private fun hintTips(): Map<String, Pair<Float, Float>> {
        val origin = IntArray(2).also { hints.getLocationInWindow(it) }
        fun box(v: View): FloatArray {
            val p = IntArray(2).also { v.getLocationInWindow(it) }
            return floatArrayOf((p[0] - origin[0]).toFloat(), (p[1] - origin[1]).toFloat(), v.width.toFloat(), v.height.toFloat())
        }
        val out = HashMap<String, Pair<Float, Float>>()
        val left = prefs.dock == NotesPrefs.Dock.LEFT
        bar.anchors.forEach { (k, v) -> val b = box(v); out[k] = (if (left) b[0] + b[2] else b[0]) to b[1] + b[3] / 2 }
        pillAnchors.forEach { (k, v) -> val b = box(v); out[k] = b[0] + b[2] / 2 to b[1] + b[3] }
        if (strip.view.visibility == View.VISIBLE) { val b = box(strip.view); out["strip"] = b[0] + b[2] / 2 to b[1] }
        return out
    }

    /** Whether a box in the hints' pixels covers any ink on the canvas. */
    private fun inkCovers(l: Float, t: Float, r: Float, b: Float): Boolean {
        val o = IntArray(2).also { hints.getLocationInWindow(it) }
        val c = IntArray(2).also { canvas.getLocationInWindow(it) }
        val v = canvas.viewport
        val boxes = canvas.inkView.strokes.map { it.bounds } + canvas.inkView.items.map { it.bounds }
        return boxes.any { w ->
            val sl = v.screenX(w.left) + c[0] - o[0]; val sr = v.screenX(w.right) + c[0] - o[0]
            val st = v.screenY(w.top) + c[1] - o[1]; val sb = v.screenY(w.bottom) + c[1] - o[1]
            sl < r && l < sr && st < b && t < sb
        }
    }

    init {
        canvas.images = images
        // F17-FR4 / F21-FR2: the pen never inks over the pill or an open panel.
        canvas.inkHoles = { listOfNotNull(zoomPill, panels.view) }
        // F17-FR13: the minimap card sits under the zoom pill when the bar leaves it that corner.
        canvas.minimapTop = { if (canvas.barOnLeft) 0 else zoomPill.bottom }
        layout()
    }

    /** Bar and column in dock order, the bar at its expanded or collapsed width: one redraw. */
    private fun layout() {
        row.removeAllViews()
        val barParams = LinearLayout.LayoutParams(if (prefs.collapsed) bar.collapsedPx else bar.expandedPx,
            LinearLayout.LayoutParams.MATCH_PARENT)
        val colParams = LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.MATCH_PARENT, 1f)
        val rule = View(a).apply { setBackgroundColor(ui.ink) }
        val ruleParams = LinearLayout.LayoutParams(maxOf(1, ui.dp(1)), LinearLayout.LayoutParams.MATCH_PARENT)
        if (prefs.dock == NotesPrefs.Dock.LEFT) {
            row.addView(bar.view, barParams); row.addView(rule, ruleParams); row.addView(column, colParams)
        } else {
            row.addView(column, colParams); row.addView(rule, ruleParams); row.addView(bar.view, barParams)
        }
        canvas.barOnLeft = prefs.dock == NotesPrefs.Dock.LEFT
        bar.onLeft = canvas.barOnLeft
        refreshBar()
    }

    /** The bar's state now; it rebuilds only when its items change, else edits in place. */
    fun refreshBar() {
        val recording = host.recordingSince() != null
        if (recording && tool == NoteTool.LISTEN) tool = NoteTool.PEN // Listen is off while recording
        val ink = canvas.inkView
        bar.render(ToolBarModel.State(tool, prefs.collapsed, recording, ink.canUndo, ink.canRedo,
            host.timer(), host.cost(), host.playing(), host.canPlay(), prefs.focus,
            lastShape = lastShape, style = ink.defaultStyle, open = panels.open, status = status,
            selection = selectionState()))
    }

    /** Opens the canvas at its last viewport, or fitted to its ink; or (F19-FR3) centred on
     *  [focus], a search hit's line, outlined with a 2 px dashed box. */
    fun open(focus: WorldRect? = null, canvasN: Int? = null) {
        // F21-FR12: a canvas other than the one shown is shown first (a search hit on it, ⋮).
        val call = { if (canvasN != null) NativeBridge.nativeCanvasShow(id, canvasN) else NativeBridge.nativeNotesOpen(id) }
        openWith(call, focus, hintsIfEmpty = canvasN == null)
    }

    private fun openWith(call: () -> String, focus: WorldRect?, hintsIfEmpty: Boolean) {
        scope.core(call) { json ->
            when (val r = CoreJson.obj(json) { it }) {
                is CoreResult.Ok -> {
                    opened = true
                    val o = r.value
                    o.str("imagesDir")?.let { images.folder = java.io.File(it) }
                    canvas.post {
                        val w = canvas.width.toDouble(); val h = canvas.height.toDouble()
                        val v = o.optJSONObject("view")
                        canvas.outline = focus
                        val next = if (focus != null) {
                            Viewport.centred((focus.left + focus.right) / 2, (focus.top + focus.bottom) / 2,
                                v?.optDouble("zoom", 1.0) ?: 1.0, w, h)
                        } else if (v != null) {
                            Viewport.centred(v.optDouble("cx"), v.optDouble("cy"), v.optDouble("zoom", 1.0), w, h)
                        } else {
                            Viewport.fitInk(InkView.rect(o.optJSONArray("bounds")), w, h, NotesCanvasView.FIT_PAD)
                        }
                        canvas.setViewport(next)
                        if (front) canvas.claimPen("open")
                        // F17-FR14: an empty canvas opens with the hints, one with ink without them.
                        if (hintsIfEmpty && o.optInt("strokes", 0) == 0 && o.optInt("items", 0) == 0) hints.show()
                    }
                }
                is CoreResult.Err -> dialogs.show { InkDialog.error(a, "The notes can't be opened", r.message) }
            }
        }
    }

    /** inkread's applyToolInkState: re-claim the pen on every resume and focus gain (the
     *  firmware drops the claim when another window takes focus), while this tab is in front. */
    fun claim(reason: String) { if (opened && front) canvas.claimPen(reason) }

    /** inkread's onPause: the claim is given back (else the pen inks other apps, #157). */
    fun pause() = canvas.releasePen()

    /** The tab came to the front or went behind the Minutes tab. */
    fun shown(visible: Boolean) {
        if (visible == front) return
        front = visible
        if (!opened) return
        if (visible) canvas.claimPen("tab") else { canvas.bake(InkBake.Event.PAGE_LEAVING); canvas.releasePen() }
    }

    /** The page goes: the pen is let go, the ink saved and the viewport kept. */
    fun close() {
        panels.close()
        if (textEditor.open) textEditor.commit()
        handler.removeCallbacks(save)
        canvas.releasePen()
        if (!opened) return
        opened = false
        val (cx, cy) = canvas.viewport.centre(canvas.width.toDouble(), canvas.height.toDouble())
        val z = canvas.viewport.zoom
        scope.core({ NativeBridge.nativeNotesClose(id, cx.toFloat(), cy.toFloat(), z.toFloat()) }) { json ->
            reportError(json, "Couldn't save the notes")
        }
    }

    /** A change to the ink: run [call], fetch what is in view (drawn once the pen rests) and
     *  arm the deferred save. */
    private fun changed(event: InkBake.Event, call: () -> String) {
        scope.core(call) { json ->
            if (!reportError(json, "Couldn't keep that")) {
                fetch(event)
                scheduleSave()
            }
        }
    }

    private fun scheduleSave() {
        handler.removeCallbacks(save)
        handler.postDelayed(save, SAVE_MS)
    }

    /** The strokes in view (plus a margin, so a small pan needs no fetch). */
    fun fetch(event: InkBake.Event?) {
        val v = canvas.viewport
        val r = v.visible(canvas.width.toDouble(), canvas.height.toDouble())
        val m = maxOf(r.width, r.height) / 2
        scope.core({
            NativeBridge.nativeCanvasForDraw(id, (r.left - m).toFloat(), (r.top - m).toFloat(),
                (r.right + m).toFloat(), (r.bottom + m).toFloat())
        }) { json ->
            CoreJson.obj(json, InkView::from).getOrNull()?.let {
                val before = canvas.inkView
                val history = it.canUndo != before.canUndo || it.canRedo != before.canRedo
                val selection = it.selection != before.selection
                canvas.show(it, event)
                target = target?.takeIf { t -> it.items.any { i -> i.ref == t.first } }
                placeMarks()
                // Undo and Redo light up or grey out; a selection swaps the tools for its actions.
                if (history || selection || it.defaultStyle != before.defaultStyle) refreshBar()
                if (event == InkBake.Event.EDIT) refreshStylePanel()
            }
        }
    }

    /** True when [json] is an error, which is then shown. */
    private fun reportError(json: String, title: String): Boolean {
        val e = CoreJson.obj(json) { o: JSONObject -> o } as? CoreResult.Err ?: return false
        dialogs.show { InkDialog.error(a, title, e.message) }
        return true
    }

    companion object {
        const val LAYER = "canvas"
        /** sn-canvas `TextElements.TEXT_WIDTH`, `NOTE_SIZE` (the core's too). */
        const val TEXT_WIDTH = 320.0
        const val STATUS_MS = 4_000L
        const val NOTE_SIDE = 200.0
        const val CLEAR_TITLE = "Clear this canvas?"
        const val CLEAR_BODY = "Everything on this canvas is removed. Undo brings it back."
        const val CLEAR_YES = "Clear canvas"
        const val SAVE_MS = 1_500L
        /** inkread's default nib (`StylusInkController.PEN_WIDTHS[2]`): matches the firmware's. */
        const val PEN_WIDTH_PX = 9f
    }
}

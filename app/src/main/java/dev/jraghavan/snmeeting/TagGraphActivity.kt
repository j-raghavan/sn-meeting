package dev.jraghavan.snmeeting

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextView
import dev.jraghavan.snmeeting.model.CoreJson
import dev.jraghavan.snmeeting.model.CoreResult
import dev.jraghavan.snmeeting.model.TagGraph
import dev.jraghavan.snmeeting.model.TagGraphMath
import dev.jraghavan.snmeeting.model.TagList
import dev.jraghavan.snmeeting.model.Tags

/**
 * F20-FR7: the Tag Graph. Each tag is a circle sized by how many recordings use it; lines
 * join tags used in the same recordings. All time / Last 3 months, Fit, ＋ / −, pinch and
 * drag. The core lays it out once (F20-FR9); this screen draws it still and redraws only when
 * the graph, the selection or the view changes (NFR2).
 */
class TagGraphActivity : Activity() {

    private lateinit var ui: Inkwell
    private lateinit var graphView: TagGraphView
    private lateinit var rangeRow: LinearLayout
    private lateinit var empty: TextView
    private var range = TagGraphMath.Range.ALL
    private var graph = TagGraph.EMPTY
    /** The selected tag's name, kept across reloads (a rename, a new range). */
    private var selectedName: String? = null
    /** The last reply drawn: the same reply again draws nothing. */
    private var drawn: String? = null
    private lateinit var listBox: LinearLayout
    private var list: TagList? = null
    private var page = 0

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        ui = Inkwell(this)
        CoreJobs.init(filesDir.absolutePath)
        selectedName = intent.getStringExtra(EXTRA_TAG)
        setContentView(buildView())
    }

    override fun onResume() {
        super.onResume()
        load()
    }

    override fun finish() {
        super.finish()
        overridePendingTransition(0, 0)
    }

    private fun buildView(): View {
        graphView = TagGraphView(this, resources.displayMetrics.density, interactive = true) { i -> select(i) }
        empty = ui.note(EMPTY, 17f, ui.textMuted).apply {
            gravity = Gravity.CENTER; typeface = ui.serifItalic; visibility = View.GONE
        }
        rangeRow = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL; gravity = Gravity.CENTER_VERTICAL }
        showRange()
        val header = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            addView(LinearLayout(this@TagGraphActivity).apply { addView(ui.backControl("Home")) },
                LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
            addView(ui.body("Tags", 24f, bold = true))
            addView(LinearLayout(this@TagGraphActivity).apply {
                gravity = Gravity.END or Gravity.CENTER_VERTICAL
                addView(ImageView(this@TagGraphActivity).apply {
                    setImageResource(R.drawable.ic_search)
                    val p = ui.dp(8); setPadding(p, p, p, p)
                    contentDescription = "Search"
                    isClickable = true
                    setOnClickListener { startActivity(Intent(this@TagGraphActivity, SearchActivity::class.java)); overridePendingTransition(0, 0) }
                }, LinearLayout.LayoutParams(ui.dp(48), ui.dp(48)))
            }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
        }
        val controls = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            addView(rangeRow)
            addView(View(this@TagGraphActivity), LinearLayout.LayoutParams(0, 1, 1f))
            addView(control("Fit") { graphView.fit() })
            addView(control("＋") { graphView.zoomBy(TagGraphMath.STEP) }, ui.wrap().apply { marginStart = ui.dim(10) })
            addView(control("−") { graphView.zoomBy(1 / TagGraphMath.STEP) }, ui.wrap().apply { marginStart = ui.dim(10) })
        }
        val card = FrameLayout(this).apply {
            background = ui.outline(ui.dim(6))
            setPadding(ui.dp(1), ui.dp(1), ui.dp(1), ui.dp(1))
            addView(graphView, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
            addView(empty, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        }
        val column = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(ui.side, ui.dim(24), ui.side, ui.dim(20))
            addView(header, ui.fullWidth())
            addView(ui.spacer(ui.dim(16)))
            addView(controls, ui.fullWidth())
            addView(ui.spacer(ui.dim(14)))
            addView(card, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
            addView(ui.eyebrow(TagGraphMath.LEGEND, ui.textMuted).apply { gravity = Gravity.CENTER }, ui.fullWidth().apply { topMargin = ui.dim(10) })
            addExtras(this)
        }
        return FrameLayout(this).apply {
            setBackgroundColor(android.graphics.Color.WHITE)
            addView(column, FrameLayout.LayoutParams(FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT))
        }
    }

    /** F20-FR8: the selected tag's recordings, under the graph. */
    private fun addExtras(column: LinearLayout) {
        listBox = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; visibility = View.GONE }
        column.addView(listBox, ui.fullWidth().apply { topMargin = ui.dim(14) })
    }

    private fun control(text: String, onClick: () -> Unit): TextView = ui.chip(text).apply {
        setPadding(ui.dim(16), ui.dim(8), ui.dim(16), ui.dim(8))
        minWidth = ui.dp(48)
        gravity = Gravity.CENTER
        isClickable = true
        setOnClickListener { onClick() }
    }

    /** All time / Last 3 months: keyline chips, the chosen one inverted. */
    private fun showRange() {
        rangeRow.removeAllViews()
        TagGraphMath.Range.values().forEachIndexed { i, r ->
            rangeRow.addView(ui.chip(r.label, filled = r == range).apply {
                setPadding(ui.dim(14), ui.dim(8), ui.dim(14), ui.dim(8))
                isClickable = true
                setOnClickListener { if (range != r) { range = r; showRange(); load() } }
            }, ui.wrap().apply { if (i > 0) marginStart = ui.dim(8) })
        }
    }

    private fun load() {
        val r = range
        CoreJobs.run({ NativeBridge.nativeTagGraph(r.id) }) { json ->
            if (isDestroyed || r != range || json == drawn) return@run
            drawn = json
            when (val g = CoreJson.obj(json, TagGraph::from)) {
                is CoreResult.Ok -> show(g.value)
                is CoreResult.Err -> { graph = TagGraph.EMPTY; graphView.show(graph); empty.text = "Couldn't read the tags: ${g.message}"; empty.visibility = View.VISIBLE }
            }
        }
    }

    private fun show(g: TagGraph) {
        graph = g
        empty.text = EMPTY
        empty.visibility = if (g.nodes.isEmpty()) View.VISIBLE else View.GONE
        graphView.selected = selectedName?.let(g::index)
        graphView.show(g)
        onSelected()
    }

    /** A tap on a circle selects it (another circle switches the list); paper does nothing. */
    private fun select(i: Int?) {
        if (i == null) return
        selectedName = graph.nodes.getOrNull(i)?.name
        graphView.selected = i
        onSelected()
    }

    /** F20-FR8: a selection lists its recordings, newest first; none hides the list. */
    private fun onSelected() {
        val name = selectedName?.takeIf { graph.index(it) != null }
        if (name == null) { list = null; listBox.visibility = View.GONE; listBox.removeAllViews(); return }
        val r = range
        CoreJobs.run({ NativeBridge.nativeTagMeetings(name, r.id) }) { json ->
            if (isDestroyed || name != selectedName || r != range) return@run
            when (val l = CoreJson.obj(json, TagList::from)) {
                is CoreResult.Ok -> { if (list?.tag != name) page = 0; list = l.value; showList() }
                is CoreResult.Err -> { list = null; listBox.removeAllViews(); listBox.addView(ui.note("Couldn't list the recordings: ${l.message}")); listBox.visibility = View.VISIBLE }
            }
        }
    }

    /** "#gpu · 5 recordings · newest first", Rename and ✕, then one page of rows. */
    private fun showList() {
        val l = list ?: return
        listBox.removeAllViews()
        listBox.visibility = View.VISIBLE
        listBox.addView(LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            addView(ui.eyebrow(l.header), LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
            addView(ui.eyebrow("Rename", ui.inkSoft).apply {
                setPadding(ui.dim(12), ui.dim(10), ui.dim(12), ui.dim(10)); isClickable = true
                setOnClickListener { rename(l.tag) }
            })
            addView(ui.body("✕", 20f).apply {
                setPadding(ui.dim(12), ui.dim(4), ui.dim(4), ui.dim(4)); isClickable = true
                contentDescription = "Close the list"
                setOnClickListener { selectedName = null; graphView.selected = null; onSelected() }
            })
        }, ui.fullWidth())
        listBox.addView(ui.hairline())
        l.page(page).forEach { m ->
            listBox.addView(LinearLayout(this).apply {
                orientation = LinearLayout.VERTICAL
                setPadding(0, ui.dim(10), 0, ui.dim(10))
                isClickable = true
                setOnClickListener {
                    startActivity(Intent(this@TagGraphActivity, MeetingActivity::class.java).putExtra(MeetingActivity.EXTRA_ID, m.id))
                    overridePendingTransition(0, 0)
                }
                addView(ui.body(m.shownTitle, 17f, bold = m.title != null).apply { maxLines = 1; ellipsize = android.text.TextUtils.TruncateAt.END })
                addView(LinearLayout(this@TagGraphActivity).apply {
                    orientation = LinearLayout.HORIZONTAL
                    addView(ui.eyebrow(m.stamp, ui.inkSoft).apply { isAllCaps = false }, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
                    if (m.tagLine.isNotEmpty()) addView(ui.eyebrow(m.tagLine, ui.inkSoft).apply { isAllCaps = false; maxLines = 1 })
                }, ui.fullWidth().apply { topMargin = ui.dim(4) })
            }, ui.fullWidth())
            listBox.addView(ui.hairline())
        }
        if (l.pages() > 1) listBox.addView(LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER
            addView(ui.body("‹", 22f).apply { setPadding(ui.dim(18), ui.dim(4), ui.dim(18), ui.dim(4)); isClickable = true; setOnClickListener { if (page > 0) { page--; showList() } } })
            addView(ui.eyebrow(l.pageLabel(page)))
            addView(ui.body("›", 22f).apply { setPadding(ui.dim(18), ui.dim(4), ui.dim(18), ui.dim(4)); isClickable = true; setOnClickListener { if (page < l.pages() - 1) { page++; showList() } } })
        }, ui.fullWidth().apply { topMargin = ui.dim(6) })
    }

    /** F20-FR4: rename from the Tag Graph; every meeting with the tag is rewritten. */
    private fun rename(tag: String) {
        InkDialog.text(this, ui, "Rename ${Tags.shown(tag)}", tag) { to ->
            if (to.isBlank()) return@text
            CoreJobs.run({ NativeBridge.nativeRenameTag(tag, to) }) { json ->
                if (isDestroyed) return@run
                when (val r = CoreJson.obj(json) { o -> o.optString("to").ifEmpty { null } }) {
                    is CoreResult.Ok -> { selectedName = r.value; drawn = null; load() }
                    is CoreResult.Err -> InkDialog.error(this, "Couldn't rename the tag", r.message)
                }
            }
        }
    }

    companion object {
        /** The tag to select on opening (a pill, a search hit, Home's panel). */
        const val EXTRA_TAG = "tag"
        const val EMPTY = "Tags appear after a meeting is summarised"

        fun open(c: Context, tag: String?) {
            c.startActivity(Intent(c, TagGraphActivity::class.java).apply { tag?.let { putExtra(EXTRA_TAG, it) } })
            (c as? Activity)?.overridePendingTransition(0, 0)
        }
    }
}

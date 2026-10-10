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

    /** Room under the graph for what a selected tag shows (F20-FR8). */
    private fun addExtras(column: LinearLayout) = Unit

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

    private fun select(i: Int?) {
        if (i == null) return
        selectedName = graph.nodes.getOrNull(i)?.name
        graphView.selected = i
        onSelected()
    }

    /** What a selection shows besides its ring (F20-FR8). */
    private fun onSelected() = Unit

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

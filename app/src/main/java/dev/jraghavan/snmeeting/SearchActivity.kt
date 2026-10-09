package dev.jraghavan.snmeeting

import android.app.Activity
import android.graphics.Typeface
import android.os.Bundle
import android.text.SpannableStringBuilder
import android.text.Spanned
import android.text.TextUtils
import android.text.style.StyleSpan
import android.view.Gravity
import android.view.KeyEvent
import android.view.View
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.TextView
import dev.jraghavan.snmeeting.model.CoreJson
import dev.jraghavan.snmeeting.model.CoreResult
import dev.jraghavan.snmeeting.model.SearchGroup
import dev.jraghavan.snmeeting.model.SearchHit
import dev.jraghavan.snmeeting.model.SearchModel
import dev.jraghavan.snmeeting.model.SearchModel.Filter
import dev.jraghavan.snmeeting.model.SearchResults

/**
 * Search (spec F19-FR1/FR2) in the Inkwell style (F9): "← Back", the query box, the filter
 * chips, then the results grouped by meeting or event. Results come in one redraw per query,
 * filter or Show more, never as a live-updating list (e-ink). Queries run on the local index
 * only (F19-FR10).
 *
 * The box is in keyboard mode (⌨). M12 hook: the ✎ ink strip (QueryInk, F19-FR1) arrives with
 * the notes surface; it renders the strip to a PNG and sends it through nativeRecogniseQuery,
 * falling back here on `{error}` with "Can't read handwriting offline — type your search".
 */
class SearchActivity : Activity() {

    private lateinit var ui: Inkwell
    private lateinit var box: EditText
    private lateinit var chips: LinearLayout
    private lateinit var status: TextView
    private lateinit var results: LinearLayout
    private var filter = Filter.ALL
    private var limit = SearchModel.PAGE_GROUPS
    /** Bumped per request, so an older answer arriving late never replaces a newer one. */
    private var asked = 0

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        ui = Inkwell(this)
        CoreJobs.init(filesDir.absolutePath)
        SearchJobs.init(filesDir.absolutePath)
        setContentView(buildView())
        box.requestFocus()
        window.setSoftInputMode(android.view.WindowManager.LayoutParams.SOFT_INPUT_STATE_VISIBLE)
    }

    override fun onResume() {
        super.onResume()
        // The files may have changed while we were away (a job, the desktop): bring the index up
        // to date first; a search queued after it on the query thread answers from what is there.
        SearchJobs.refresh()
        if (box.text.isNotBlank()) search()
    }

    override fun finish() {
        super.finish()
        overridePendingTransition(0, 0)
    }

    private fun buildView(): View {
        box = ui.field("").apply {
            hint = "Search notes, transcripts, minutes, to-dos and events"
            imeOptions = EditorInfo.IME_ACTION_SEARCH
            inputType = android.text.InputType.TYPE_CLASS_TEXT
            setOnEditorActionListener { _, action, ev ->
                val enter = ev?.keyCode == KeyEvent.KEYCODE_ENTER && ev.action == KeyEvent.ACTION_DOWN
                if (action == EditorInfo.IME_ACTION_SEARCH || enter) { limit = SearchModel.PAGE_GROUPS; search(); true } else false
            }
        }
        chips = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL; gravity = Gravity.CENTER }
        status = ui.note("", 14f).apply { gravity = Gravity.CENTER; visibility = View.GONE }
        results = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        showChips()
        showHint()
        val column = ui.column().apply {
            addView(ui.header("Back"))
            addView(ui.spacer(ui.dim(32)))
            addView(LinearLayout(this@SearchActivity).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                addView(box, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
                addView(ui.pill("Search", filled = true, size = 16f) { limit = SearchModel.PAGE_GROUPS; search() },
                    ui.wrap().apply { marginStart = ui.dim(12) })
            }, ui.fullWidth())
            addView(ui.spacer(ui.dim(18)))
            addView(chips, ui.fullWidth())
            addView(ui.spacer(ui.dim(14)))
            addView(status, ui.fullWidth())
            addView(ui.spacer(ui.dim(10)))
            addView(results, ui.fullWidth())
            addView(ui.spacerWeighted(ui.phi))
            addView(ui.spacer(ui.dim(32)))
            addView(ui.closingMark())
        }
        return ui.page(column)
    }

    /** All / Notes / Transcript / Minutes / To-dos / Events: keyline chips, the chosen one inverted. */
    private fun showChips() {
        chips.removeAllViews()
        Filter.values().forEachIndexed { i, f ->
            chips.addView(ui.chip(f.label, filled = f == filter).apply {
                textSize = ui.fs(12f)
                setPadding(ui.dim(14), ui.dim(8), ui.dim(14), ui.dim(8))
                isClickable = true
                setOnClickListener {
                    if (filter != f) { filter = f; limit = SearchModel.PAGE_GROUPS; showChips(); search() }
                }
            }, ui.wrap().apply { if (i > 0) marginStart = ui.dim(8) })
        }
    }

    private fun showHint() {
        results.removeAllViews()
        results.addView(ui.note("Type a word and tap Search. A word's beginning is enough: “budg” finds “budget”.", 15f).apply {
            gravity = Gravity.CENTER; setPadding(0, ui.dim(24), 0, 0)
        }, ui.fullWidth())
    }

    private fun search() {
        val q = box.text.toString()
        if (q.isBlank()) { status.visibility = View.GONE; return showHint() }
        getSystemService(InputMethodManager::class.java)?.hideSoftInputFromWindow(box.windowToken, 0)
        val (f, n, mine) = Triple(filter, limit, ++asked)
        SearchJobs.query({ NativeBridge.nativeSearch(q, f.id, n) }) { json ->
            if (isDestroyed || mine != asked) return@query
            when (val r = CoreJson.obj(json, SearchResults::from)) {
                is CoreResult.Ok -> show(r.value, q, f)
                is CoreResult.Err -> showMessage("Search didn't work: ${r.message}")
            }
        }
    }

    private fun showMessage(text: String) {
        status.visibility = View.GONE
        results.removeAllViews()
        results.addView(ui.note(text, 15f).apply { gravity = Gravity.CENTER; setPadding(0, ui.dim(24), 0, 0) }, ui.fullWidth())
    }

    /** One redraw: the whole list is built off-screen, then swapped in. */
    private fun show(r: SearchResults, q: String, f: Filter) {
        status.text = "Still indexing…"
        status.visibility = if (r.indexing) View.VISIBLE else View.GONE
        if (r.groups.isEmpty()) return showMessage(SearchModel.noResults(q, f))
        val list = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        r.groups.forEach { g -> list.addView(group(g), ui.fullWidth().apply { topMargin = ui.dim(22) }) }
        if (r.more) {
            list.addView(ui.pill("Show more", size = 15f) { limit += SearchModel.PAGE_GROUPS; search() },
                ui.wrap().apply { topMargin = ui.dim(24); gravity = Gravity.CENTER_HORIZONTAL })
        }
        results.removeAllViews()
        results.addView(list, ui.fullWidth())
    }

    /** Group header (title, then the date and time in mono), then its hits in one card. */
    private fun group(g: SearchGroup): View = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        addView(ui.body(SearchModel.title(g), 18f, bold = true).apply { maxLines = 2; ellipsize = TextUtils.TruncateAt.END })
        addView(ui.eyebrow(SearchModel.stamp(g.startMs), ui.inkSoft), ui.wrap().apply { topMargin = ui.dim(4) })
        addView(ui.card(padV = ui.dim(6)).apply {
            g.hits.forEachIndexed { i, h ->
                if (i > 0) addView(ui.hairline())
                addView(hit(h).apply { isClickable = true; setOnClickListener { open(SearchModel.open(g, h)) } }, ui.fullWidth())
            }
            SearchModel.more(g)?.let { addView(ui.note(it, 13f, ui.textMuted).apply { setPadding(0, ui.dim(4), 0, ui.dim(8)) }) }
        }, ui.fullWidth().apply { topMargin = ui.dim(10) })
    }

    /** Source label (and ▶ time), then up to two lines of snippet with the matches in bold. */
    private fun hit(h: SearchHit): View = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(0, ui.dim(12), 0, ui.dim(12))
        addView(ui.eyebrow(SearchModel.label(h)))
        addView(ui.body(snippet(h), 16f).apply {
            maxLines = 2; ellipsize = TextUtils.TruncateAt.END; setLineSpacing(0f, 1.25f)
        }, ui.fullWidth().apply { topMargin = ui.dim(6) })
    }

    /** F19-FR3: a meeting hit opens the meeting at its moment or part; an event its details. */
    private fun open(o: SearchModel.Open) {
        when (o) {
            is SearchModel.Open.Meeting -> {
                val i = android.content.Intent(this, MeetingActivity::class.java).putExtra(MeetingActivity.EXTRA_ID, o.id)
                o.atMs?.let { i.putExtra(MeetingActivity.EXTRA_AT_MS, it) }
                o.focus?.let { i.putExtra(MeetingActivity.EXTRA_FOCUS, it) }
                startActivity(i)
                overridePendingTransition(0, 0)
            }
            // F18-FR8: the event's sheet; "● Record" goes back to Home, which records it.
            is SearchModel.Open.Event -> EventSheetDialog.open(this, ui, o.key, "The microphone Home shows") { key, template ->
                startActivity(android.content.Intent(this, MainActivity::class.java)
                    .addFlags(android.content.Intent.FLAG_ACTIVITY_CLEAR_TOP or android.content.Intent.FLAG_ACTIVITY_SINGLE_TOP)
                    .putExtra(MainActivity.EXTRA_RECORD_EVENT, key).putExtra(MainActivity.EXTRA_TEMPLATE, template))
                overridePendingTransition(0, 0)
            }
        }
    }

    private fun snippet(h: SearchHit): CharSequence = SpannableStringBuilder().apply {
        h.snippet.forEach { s ->
            val start = length
            append(s.text)
            if (s.bold) setSpan(StyleSpan(Typeface.BOLD), start, length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        }
    }
}

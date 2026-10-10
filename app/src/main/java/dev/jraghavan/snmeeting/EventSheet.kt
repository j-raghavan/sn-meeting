package dev.jraghavan.snmeeting

import android.app.Activity
import android.app.AlertDialog
import android.text.TextUtils
import android.view.Gravity
import android.view.ViewGroup
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import dev.jraghavan.snmeeting.model.CalendarFormat
import dev.jraghavan.snmeeting.model.CoreJson
import dev.jraghavan.snmeeting.model.CoreResult
import dev.jraghavan.snmeeting.model.EventSheet
import dev.jraghavan.snmeeting.model.Format
import dev.jraghavan.snmeeting.model.TemplateRow
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale

/**
 * F18-FR8: an event's sheet, an Inkwell card over the lower half of the screen: the event's
 * details (the agenda as plain text; links are shown, not opened), the template its title picks
 * (changeable), the mic line, "● Record this meeting" and "Earlier in this series". Home and
 * Search open it; [onRecord] starts the recording (Home owns that flow).
 */
object EventSheetDialog {

    /** Read the event, then show its sheet; a gone event says so. */
    fun open(a: Activity, ui: Inkwell, eventKey: String, micLine: String, onRecord: (key: String, templateId: String) -> Unit) {
        CoreJobs.run({ NativeBridge.nativeEvent(eventKey) }) { json ->
            if (a.isFinishing || a.isDestroyed) return@run
            when (val r = CoreJson.obj(json, EventSheet::from)) {
                is CoreResult.Err -> InkDialog.error(a, "Event", r.message)
                is CoreResult.Ok -> CoreJobs.run({ NativeBridge.nativeListTemplates() }) { tj ->
                    if (a.isFinishing || a.isDestroyed) return@run
                    val templates = CoreJson.list(tj, TemplateRow::from).getOrNull().orEmpty()
                    show(a, ui, r.value, templates, micLine, onRecord)
                }
            }
        }
    }

    private fun show(a: Activity, ui: Inkwell, s: EventSheet, templates: List<TemplateRow>, micLine: String, onRecord: (String, String) -> Unit) {
        val zone = ZoneId.systemDefault()
        var template = s.templateId
        lateinit var dialog: AlertDialog
        val templateLine = ui.body("", 16f)
        fun showTemplate() {
            val name = templates.firstOrNull { it.id == template }?.name ?: template
            templateLine.text = if (s.templateKeyword != null && template == s.templateId) "$name  ·  matched “${s.templateKeyword}”" else name
        }
        showTemplate()
        val card = LinearLayout(a).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(ui.dim(28), ui.dim(22), ui.dim(28), ui.dim(22))
            addView(ui.eyebrow(listOfNotNull(s.calendar, if (s.recurring) "Recurring" else null).joinToString("  ·  ").ifEmpty { "Event" }, ui.inkSoft))
            addView(ui.body(s.title.ifBlank { "(No title)" }, 24f, bold = true), ui.fullWidth().apply { topMargin = ui.dim(6) })
            val day = Instant.ofEpochMilli(s.startMs).atZone(zone).format(DateTimeFormatter.ofPattern("EEEE d MMMM", Locale.UK))
            val time = if (s.allDay) "All day" else CalendarFormat.range(s.startMs, s.endMs, zone)
            addView(ui.body("$day  ·  $time", 16f), ui.fullWidth().apply { topMargin = ui.dim(6) })
            if (s.tzUnknown) addView(ui.note("Time zone unknown: shown in this device's time.", 13f), ui.fullWidth())
            s.location?.let { addView(ui.note(it, 15f), ui.fullWidth().apply { topMargin = ui.dim(4) }) }
            if (s.attendees.isNotEmpty()) addView(ui.note(s.attendees.joinToString(", "), 14f).apply {
                maxLines = 3; ellipsize = TextUtils.TruncateAt.END
            }, ui.fullWidth().apply { topMargin = ui.dim(4) })
            s.agenda?.let {
                addView(ui.eyebrow("Agenda"), ui.fullWidth().apply { topMargin = ui.dim(14) })
                // Plain text: links are shown, never opened (F18-FR8).
                addView(ui.body(it, 15f).apply { maxLines = 8; ellipsize = TextUtils.TruncateAt.END; setLineSpacing(0f, 1.2f) },
                    ui.fullWidth().apply { topMargin = ui.dim(4) })
            }
            addView(ui.hairline(), ui.fullWidth().apply { topMargin = ui.dim(16) })
            addView(LinearLayout(a).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                setPadding(0, ui.dim(12), 0, ui.dim(12))
                addView(ui.eyebrow("Template"), LinearLayout.LayoutParams(ui.dim(130), LinearLayout.LayoutParams.WRAP_CONTENT))
                addView(templateLine, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
                addView(ui.eyebrow("Change  ›"))
                isClickable = templates.size > 1
                setOnClickListener {
                    val names = templates.map { it.name }.toTypedArray()
                    AlertDialog.Builder(a, R.style.InkDialog).setTitle("Template")
                        .setItems(names) { _, i -> template = templates[i].id; showTemplate() }
                        .setNegativeButton("CANCEL", null).show()
                }
            })
            addView(LinearLayout(a).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                setPadding(0, 0, 0, ui.dim(12))
                addView(ui.eyebrow("Microphone"), LinearLayout.LayoutParams(ui.dim(130), LinearLayout.LayoutParams.WRAP_CONTENT))
                addView(ui.body(micLine, 16f), LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
            })
            addView(ui.pill("●  Record this meeting", filled = true, size = 18f) {
                dialog.dismiss(); onRecord(s.eventKey, template)
            }, ui.wrap().apply { gravity = Gravity.CENTER_HORIZONTAL; topMargin = ui.dim(4) })
            if (s.fromLastTime.isNotEmpty()) {
                addView(ui.eyebrow("From last time"), ui.fullWidth().apply { topMargin = ui.dim(18) })
                s.fromLastTime.forEach { t -> addView(ui.body("☐  ${t.text}", 15f), ui.fullWidth().apply { topMargin = ui.dim(4) }) }
            }
            if (s.recurring) {
                addView(ui.eyebrow("Earlier in this series"), ui.fullWidth().apply { topMargin = ui.dim(18) })
                if (s.series.isEmpty()) addView(ui.note("No earlier meetings recorded.", 14f), ui.fullWidth().apply { topMargin = ui.dim(4) })
                s.series.forEach { m ->
                    addView(TextView(a).apply {
                        val todos = if (m.openTodos > 0) "  ·  " + Format.plural(m.openTodos, "open to-do") else ""
                        text = "${Format.day(m.startedAt)}  ·  ${m.title ?: Format.UNTITLED}$todos  →"
                        textSize = ui.fs(15f); typeface = ui.serif; setTextColor(ui.ink)
                        setPadding(0, ui.dim(8), 0, ui.dim(8))
                        isClickable = true
                        setOnClickListener {
                            dialog.dismiss()
                            a.startActivity(android.content.Intent(a, MeetingActivity::class.java).putExtra(MeetingActivity.EXTRA_ID, m.meetingId))
                            a.overridePendingTransition(0, 0)
                        }
                    }, ui.fullWidth())
                }
            }
        }
        val scroll = ScrollView(a).apply {
            isVerticalScrollBarEnabled = false
            addView(card, ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        }
        dialog = AlertDialog.Builder(a, R.style.InkDialog).setView(scroll).setNegativeButton("CLOSE", null).create()
        dialog.window?.let { w ->
            w.setGravity(Gravity.BOTTOM)
            w.setWindowAnimations(0) // e-ink: no animation
        }
        dialog.show()
        dialog.window?.setLayout(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
    }
}

/**
 * F18-FR10: "Link to event…": the events of the meeting's day, the ones overlapping the
 * recording first, and "Unlink" when it is linked. [done] runs after the core answered.
 */
object EventLinkPicker {
    fun show(a: Activity, meetingId: String, startMs: Long, endMs: Long, linkedKey: String?, done: () -> Unit) {
        val day = java.time.Instant.ofEpochMilli(startMs).atZone(ZoneId.systemDefault()).toLocalDate().toString()
        CoreJobs.run({ NativeBridge.nativeAgenda(day, day) }) { json ->
            if (a.isFinishing || a.isDestroyed) return@run
            val d = CoreJson.obj(json, dev.jraghavan.snmeeting.model.Agenda::from).getOrNull()?.days?.firstOrNull()
            val events = CalendarFormat.pickerOrder(CalendarFormat.linkable(d?.rows.orEmpty()), startMs, endMs)
            val zone = ZoneId.systemDefault()
            val labels = events.map { e ->
                val time = if (e.allDay) "All day" else CalendarFormat.range(e.startMs, e.endMs, zone)
                (if (e.eventKey == linkedKey) "✓  " else "") + "${CalendarFormat.rowTitle(e)}  ·  $time"
            } + listOfNotNull(if (linkedKey != null) "Unlink (unscheduled)" else null)
            if (labels.isEmpty()) return@run InkDialog.info(a, "Link to event", "There are no calendar events on this day.")
            AlertDialog.Builder(a, R.style.InkDialog).setTitle("Link to event")
                .setItems(labels.toTypedArray()) { _, i ->
                    val key = events.getOrNull(i)?.eventKey ?: ""
                    CoreJobs.run({ NativeBridge.nativeLinkEvent(meetingId, key) }) { r ->
                        (CoreJson.ok(r) as? CoreResult.Err)?.let { InkDialog.error(a, "Couldn't link it", it.message) }
                        done()
                    }
                }
                .setNegativeButton("CANCEL", null).show()
        }
    }
}
